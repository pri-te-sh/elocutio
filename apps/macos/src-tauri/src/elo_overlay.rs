//! Single-process system-wide `"elo"` floating widget & inline underline overlay.
//!
//! Replaces the legacy child-process `winit`/`egui`/`wgpu` overlay with a transparent,
//! non-activating, always-on-top Tauri `WebviewWindow` (`label = "elo-overlay"`) backed by
//! a dedicated macOS Accessibility polling & cursor hit-testing thread.

use std::collections::HashMap;
use std::sync::{
    Arc, Mutex as StdMutex, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Sender},
};
use std::thread;
use std::time::{Duration, Instant};

use harper_core::{
    Document,
    linting::{LintKind, Suggestion},
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tokio::sync::Mutex as AsyncMutex;

use crate::config::Config;
use crate::os_broker::{AccessibilityPermissionStatus, OsBroker};
use crate::rect::{ActionableLint, Rect};

/// Logical screen-space rectangle serialized to the frontend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct EloRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl EloRect {
    pub fn from_rect(r: Rect, origin_x: f64, origin_y: f64) -> Self {
        Self {
            x: r.x - origin_x,
            y: r.y - origin_y,
            width: r.width,
            height: r.height,
        }
    }

    pub fn contains(&self, px: f64, py: f64, pad: f64) -> bool {
        px >= self.x - pad
            && px <= self.x + self.width + pad
            && py >= self.y - pad
            && py <= self.y + self.height + pad
    }
}

/// One replacement option for an alert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EloSuggestion {
    pub kind: String,
    pub label: String,
    pub replacement: String,
}

/// One lint/editorial alert positioned in the active text area.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EloAlert {
    pub id: String,
    pub rule_name: String,
    pub category: String,
    pub category_label: String,
    pub subcategory: String,
    pub color: String,
    pub original: String,
    pub message: String,
    pub context_before: String,
    pub context_after: String,
    pub suggestions: Vec<EloSuggestion>,
    pub char_start: usize,
    pub char_end: usize,
    pub rect: EloRect,
}

/// Complete snapshot of the system-wide `"elo"` overlay state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EloOverlaySnapshot {
    pub service_enabled: bool,
    pub ax_trusted: bool,
    pub field_active: bool,
    pub pid: i32,
    pub app_bundle_id: String,
    pub app_name: String,
    pub field_rect: Option<EloRect>,
    pub widget_rect: Option<EloRect>,
    pub alerts: Vec<EloAlert>,
    pub tone: Option<String>,
    pub score: u32,
    pub word_count: usize,
    pub source_text: String,
    pub card_command: Option<String>,
}

/// Studio navigation request when opening the main window from the `"elo"` overlay or tray.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StudioLaunchState {
    pub requested_tab: Option<String>,
    pub imported_text: Option<String>,
    pub imported_app_name: Option<String>,
}

enum OverlayCommand {
    ApplyAlert {
        alert_id: String,
        suggestion_index: usize,
    },
    DismissAlert {
        alert_id: String,
    },
    ReplaceFieldText {
        new_text: String,
    },
    RefreshNow,
}

struct SharedOverlayState {
    snapshot: EloOverlaySnapshot,
    frontend_widget_rect: Option<EloRect>,
    frontend_card_rect: Option<EloRect>,
    is_dragging: bool,
    widget_offsets: HashMap<String, (f64, f64)>,
    monitor_origin: (f64, f64),
    monitor_size: (f64, f64),
    main_display_height: f64,
    studio_launch: StudioLaunchState,
    command_tx: Option<Sender<OverlayCommand>>,
}

impl Default for SharedOverlayState {
    fn default() -> Self {
        Self {
            snapshot: EloOverlaySnapshot {
                service_enabled: true,
                score: 100,
                ..Default::default()
            },
            frontend_widget_rect: None,
            frontend_card_rect: None,
            is_dragging: false,
            widget_offsets: HashMap::new(),
            monitor_origin: (0.0, 0.0),
            monitor_size: (1728.0, 1117.0),
            main_display_height: 1117.0,
            studio_launch: StudioLaunchState::default(),
            command_tx: None,
        }
    }
}

static OVERLAY_STATE: OnceLock<StdMutex<SharedOverlayState>> = OnceLock::new();
static OVERLAY_APP: OnceLock<AppHandle> = OnceLock::new();
static OVERLAY_RUNNING: AtomicBool = AtomicBool::new(false);
static IGNORING_CURSOR: AtomicBool = AtomicBool::new(true);
static OVERLAY_INPUT_FOCUSED: AtomicBool = AtomicBool::new(false);

fn shared_state() -> &'static StdMutex<SharedOverlayState> {
    OVERLAY_STATE.get_or_init(|| StdMutex::new(SharedOverlayState::default()))
}

/// Whether the in-process `"elo"` overlay service is currently enabled and running.
pub fn is_running() -> bool {
    OVERLAY_RUNNING.load(Ordering::Relaxed)
}

/// Whether the main `Elocutio — Writing Studio & Settings` window (`label = "settings"`) is currently focused.
pub fn is_studio_window_focused() -> bool {
    if OVERLAY_INPUT_FOCUSED.load(Ordering::Relaxed) {
        return false;
    }
    let Some(app) = OVERLAY_APP.get() else {
        return false;
    };
    app.get_webview_window("settings")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

/// Temporarily enables or releases keyboard focus on the `"elo-overlay"` window so the user
/// can type into `"Tell elo to..."` without raising the Studio window in front of the target app.
pub fn set_overlay_input_focus(focused: bool) {
    OVERLAY_INPUT_FOCUSED.store(focused, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    {
        let Some(app) = OVERLAY_APP.get() else {
            return;
        };
        let app_for_main = app.clone();
        let target_pid = shared_state().lock().map(|g| g.snapshot.pid).unwrap_or(0);
        let _ = app.run_on_main_thread(move || {
            let app = app_for_main;
            use objc2::rc::Retained;
            use objc2::{msg_send, sel};
            use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWindow};

            if focused {
                // Keep the Studio window ("settings") at level -1 while the overlay input
                // has focus so activating Elocutio never raises Studio in front of the target app.
                if let Some(settings_win) = app.get_webview_window("settings")
                    && let Ok(ptr) = settings_win.ns_window()
                    && let Some(settings_ns) = unsafe { Retained::retain(ptr as *mut NSWindow) }
                {
                    settings_ns.resignKeyWindow();
                    settings_ns.resignMainWindow();
                    settings_ns.setLevel(-1);
                }

                if let Some(overlay_win) = app.get_webview_window("elo-overlay") {
                    let _ = overlay_win.set_ignore_cursor_events(false);
                    IGNORING_CURSOR.store(false, Ordering::Relaxed);
                    if let Ok(ptr) = overlay_win.ns_window()
                        && let Some(ns_window) = unsafe { Retained::retain(ptr as *mut NSWindow) }
                    {
                        unsafe {
                            let responds: bool = msg_send![
                                &*ns_window,
                                respondsToSelector: sel!(_setPreventsActivation:)
                            ];
                            if responds {
                                let _: () = msg_send![&*ns_window, _setPreventsActivation: false];
                            }
                        }
                        let current_app = NSRunningApplication::currentApplication();
                        #[allow(deprecated)]
                        let _ = current_app.activateWithOptions(
                            NSApplicationActivationOptions::ActivateIgnoringOtherApps,
                        );
                        ns_window.makeKeyAndOrderFront(None);
                        ns_window.makeKeyWindow();

                        if let Some(content_view) = ns_window.contentView() {
                            let subviews = content_view.subviews();
                            let mut focused_view = false;
                            for i in 0..subviews.len() {
                                let sub = subviews.objectAtIndex(i);
                                let is_wk: bool = unsafe {
                                    msg_send![
                                        &*sub,
                                        respondsToSelector: sel!(evaluateJavaScript:completionHandler:)
                                    ]
                                };
                                if is_wk {
                                    ns_window.makeFirstResponder(Some(&sub));
                                    focused_view = true;
                                    break;
                                }
                            }
                            if !focused_view {
                                if let Some(first_sub) = subviews.firstObject() {
                                    ns_window.makeFirstResponder(Some(&first_sub));
                                } else {
                                    ns_window.makeFirstResponder(Some(&content_view));
                                }
                            }
                        }
                    }
                    let _ = overlay_win.set_focus();
                    let _ = app.emit("elo-overlay-input-ready", true);
                }
            } else {
                if let Some(overlay_win) = app.get_webview_window("elo-overlay")
                    && let Ok(ptr) = overlay_win.ns_window()
                    && let Some(ns_window) = unsafe { Retained::retain(ptr as *mut NSWindow) }
                {
                    unsafe {
                        let responds: bool = msg_send![
                            &*ns_window,
                            respondsToSelector: sel!(_setPreventsActivation:)
                        ];
                        if responds {
                            let _: () = msg_send![&*ns_window, _setPreventsActivation: true];
                        }
                    }
                    ns_window.resignKeyWindow();
                }

                if target_pid > 0
                    && let Some(target_app) =
                        NSRunningApplication::runningApplicationWithProcessIdentifier(target_pid)
                {
                    #[allow(deprecated)]
                    let _ = target_app.activateWithOptions(
                        NSApplicationActivationOptions::ActivateIgnoringOtherApps,
                    );
                }

                if let Some(settings_win) = app.get_webview_window("settings")
                    && let Ok(ptr) = settings_win.ns_window()
                    && let Some(settings_ns) = unsafe { Retained::retain(ptr as *mut NSWindow) }
                {
                    settings_ns.setLevel(0);
                }
            }
        });
    }
}

/// Sets whether the `"elo"` overlay service is active.
pub fn set_running(enabled: bool) {
    OVERLAY_RUNNING.store(enabled, Ordering::Relaxed);
    if let Ok(mut guard) = shared_state().lock() {
        guard.snapshot.service_enabled = enabled;
        if !enabled {
            guard.snapshot.field_active = false;
            guard.snapshot.alerts.clear();
            guard.frontend_card_rect = None;
        }
        if let Some(tx) = &guard.command_tx {
            let _ = tx.send(OverlayCommand::RefreshNow);
        }
    }
}

/// Returns the latest `"elo"` overlay snapshot.
pub fn current_snapshot() -> EloOverlaySnapshot {
    shared_state()
        .lock()
        .map(|s| s.snapshot.clone())
        .unwrap_or_default()
}

/// Takes any pending studio launch state (requested tab or imported text).
pub fn take_studio_launch_state() -> StudioLaunchState {
    let Ok(mut guard) = shared_state().lock() else {
        return StudioLaunchState::default();
    };
    std::mem::take(&mut guard.studio_launch)
}

/// Sets the studio launch state before opening the main window.
pub fn set_studio_launch_state(tab: Option<String>, import_focused_text: bool) {
    if let Ok(mut guard) = shared_state().lock() {
        let (imported_text, imported_app_name) =
            if import_focused_text && !guard.snapshot.source_text.trim().is_empty() {
                (
                    Some(guard.snapshot.source_text.clone()),
                    Some(guard.snapshot.app_name.clone()),
                )
            } else {
                (None, None)
            };
        guard.studio_launch = StudioLaunchState {
            requested_tab: tab,
            imported_text,
            imported_app_name,
        };
    }
}

/// Updates the interactive hit regions reported by the `"elo-overlay"` frontend.
pub fn update_interactive_regions(
    widget_rect: Option<EloRect>,
    card_rect: Option<EloRect>,
    is_dragging: bool,
) {
    if let Ok(mut guard) = shared_state().lock() {
        if guard.frontend_card_rect.is_some() != card_rect.is_some() {
            eprintln!("elo overlay card_rect updated: {:?}", card_rect);
        }
        guard.frontend_widget_rect = widget_rect;
        guard.frontend_card_rect = card_rect;
        guard.is_dragging = is_dragging;
    }
    if is_dragging && let Some(app) = OVERLAY_APP.get() {
        set_window_ignore_cursor(app, false);
    }
}

/// Opens the main Elocutio Studio window and records the requested tab/imported text.
pub fn open_studio_window(tab: Option<String>, import_focused_text: bool) -> Result<(), String> {
    set_studio_launch_state(tab, import_focused_text);
    let Some(app) = OVERLAY_APP.get() else {
        return Err("AppHandle not initialized.".to_string());
    };
    crate::windows::show_settings_window(app).map_err(|e| e.to_string())
}

/// Saves the user-dragged `(dx, dy)` offset for the `"elo"` floating widget in an app.
pub fn save_widget_offset(bundle_id: &str, offset_x: f64, offset_y: f64) {
    let key = if bundle_id.trim().is_empty() {
        "default".to_string()
    } else {
        bundle_id.trim().to_string()
    };
    if let Ok(mut guard) = shared_state().lock() {
        guard.widget_offsets.insert(key, (offset_x, offset_y));
        if let Some(field_rect) = guard.snapshot.field_rect {
            let (mon_w, mon_h) = guard.monitor_size;
            guard.snapshot.widget_rect = Some(compute_widget_rect(
                field_rect, offset_x, offset_y, mon_w, mon_h,
            ));
        }
    }
}

/// Sends an ApplyAlert command to the overlay worker thread.
pub fn request_apply_alert(alert_id: String, suggestion_index: usize) -> Result<bool, String> {
    let tx = shared_state()
        .lock()
        .map_err(|e| e.to_string())?
        .command_tx
        .clone()
        .ok_or_else(|| "Overlay worker is not running.".to_string())?;
    tx.send(OverlayCommand::ApplyAlert {
        alert_id,
        suggestion_index,
    })
    .map_err(|e| format!("Failed to send apply command: {e}"))?;
    Ok(true)
}

/// Sends a DismissAlert command to the overlay worker thread.
pub fn request_dismiss_alert(alert_id: String) -> Result<bool, String> {
    let tx = shared_state()
        .lock()
        .map_err(|e| e.to_string())?
        .command_tx
        .clone()
        .ok_or_else(|| "Overlay worker is not running.".to_string())?;
    tx.send(OverlayCommand::DismissAlert { alert_id })
        .map_err(|e| format!("Failed to send dismiss command: {e}"))?;
    Ok(true)
}

/// Sends a ReplaceFieldText command to the overlay worker thread to replace the entire field text.
pub fn request_replace_field_text(new_text: String) -> Result<bool, String> {
    let tx = shared_state()
        .lock()
        .map_err(|e| e.to_string())?
        .command_tx
        .clone()
        .ok_or_else(|| "Overlay worker is not running.".to_string())?;
    tx.send(OverlayCommand::ReplaceFieldText { new_text })
        .map_err(|e| format!("Failed to send replace command: {e}"))?;
    Ok(true)
}

/// Triggers an immediate re-poll on the overlay worker thread.
pub fn request_refresh() {
    if let Ok(guard) = shared_state().lock()
        && let Some(tx) = &guard.command_tx
    {
        let _ = tx.send(OverlayCommand::RefreshNow);
    }
}

/// Creates the transparent always-on-top `"elo-overlay"` window and starts the background polling thread.
pub fn start(
    app: AppHandle,
    config: Arc<AsyncMutex<Config>>,
    suggestion_count: Arc<crate::config::SuggestionCount>,
) {
    let _ = OVERLAY_APP.set(app.clone());
    let enabled = config
        .try_lock()
        .map(|cfg| cfg.highlighter_service_enabled)
        .unwrap_or(true);
    OVERLAY_RUNNING.store(enabled, Ordering::Relaxed);

    if let Err(err) = ensure_overlay_window(&app) {
        tracing::error!("Could not create elo-overlay window: {err}");
    }

    let (command_tx, command_rx) = mpsc::channel::<OverlayCommand>();
    if let Ok(mut guard) = shared_state().lock() {
        guard.snapshot.service_enabled = enabled;
        guard.command_tx = Some(command_tx);
    }

    thread::Builder::new()
        .name("elocutio-elo-overlay-loop".to_string())
        .spawn(move || {
            run_overlay_loop(app, config, suggestion_count, command_rx);
        })
        .expect("failed to spawn elo-overlay loop thread");
}

/// Builds and configures the transparent, non-activating, always-on-top `"elo-overlay"` window.
pub fn ensure_overlay_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(existing) = app.get_webview_window("elo-overlay") {
        return Ok(existing);
    }

    let (mon_x, mon_y, mon_w, mon_h, main_h) = active_display_quartz_bounds(app, None, None);
    if let Ok(mut guard) = shared_state().lock() {
        guard.monitor_origin = (mon_x, mon_y);
        guard.monitor_size = (mon_w, mon_h);
        guard.main_display_height = main_h;
    }

    let window = WebviewWindowBuilder::new(
        app,
        "elo-overlay",
        WebviewUrl::App("index.html#elo-overlay".into()),
    )
    .title("elo")
    .position(mon_x, mon_y)
    .inner_size(mon_w, mon_h)
    .transparent(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .accept_first_mouse(true)
    .resizable(false)
    .shadow(false)
    .visible(false)
    .build()?;

    let _ = window.set_ignore_cursor_events(true);
    IGNORING_CURSOR.store(true, Ordering::Relaxed);

    let _ = window.show();

    #[cfg(target_os = "macos")]
    configure_macos_overlay_nswindow(app, &window, mon_x, mon_y, mon_w, mon_h, main_h);

    Ok(window)
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct CGPointRaw {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct CGSizeRaw {
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct CGRectRaw {
    origin: CGPointRaw,
    size: CGSizeRaw,
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceButtonState(state_id: u32, button: u32) -> bool;
    fn CGMainDisplayID() -> u32;
    fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut u32,
        display_count: *mut u32,
    ) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRectRaw;
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn objc_getClass(name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    fn class_replaceMethod(
        cls: *mut std::ffi::c_void,
        name: objc2::runtime::Sel,
        imp: unsafe extern "C" fn(
            *mut objc2_app_kit::NSWindow,
            objc2::runtime::Sel,
        ) -> objc2::runtime::Bool,
        types: *const std::ffi::c_char,
    ) -> *const std::ffi::c_void;
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn nswindow_is_nonactivating_panel(
    this: *mut objc2_app_kit::NSWindow,
    _cmd: objc2::runtime::Sel,
) -> objc2::runtime::Bool {
    if this.is_null() {
        return objc2::runtime::Bool::NO;
    }
    let level = unsafe { (*this).level() };
    if level == 25 {
        objc2::runtime::Bool::YES
    } else {
        objc2::runtime::Bool::NO
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn nswindow_can_become_key(
    this: *mut objc2_app_kit::NSWindow,
    _cmd: objc2::runtime::Sel,
) -> objc2::runtime::Bool {
    if this.is_null() {
        return objc2::runtime::Bool::NO;
    }
    let level = unsafe { (*this).level() };
    if level == 25 {
        if OVERLAY_INPUT_FOCUSED.load(Ordering::Relaxed) {
            objc2::runtime::Bool::YES
        } else {
            objc2::runtime::Bool::NO
        }
    } else if OVERLAY_INPUT_FOCUSED.load(Ordering::Relaxed) {
        objc2::runtime::Bool::NO
    } else {
        objc2::runtime::Bool::YES
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn nswindow_can_become_main(
    this: *mut objc2_app_kit::NSWindow,
    _cmd: objc2::runtime::Sel,
) -> objc2::runtime::Bool {
    if this.is_null() {
        return objc2::runtime::Bool::NO;
    }
    let level = unsafe { (*this).level() };
    if level == 25 || OVERLAY_INPUT_FOCUSED.load(Ordering::Relaxed) {
        objc2::runtime::Bool::NO
    } else {
        objc2::runtime::Bool::YES
    }
}

/// Returns `(mon_x, mon_y, mon_w, mon_h, main_display_height)` in global Quartz coordinates
/// for the display containing `target_rect` (or `fallback_cursor`, or the main display).
#[cfg(target_os = "macos")]
fn active_display_quartz_bounds(
    _app: &AppHandle,
    target_rect: Option<Rect>,
    fallback_cursor: Option<egui::Pos2>,
) -> (f64, f64, f64, f64, f64) {
    let main_id = unsafe { CGMainDisplayID() };
    let main_bounds = unsafe { CGDisplayBounds(main_id) };
    let main_h = main_bounds.size.height.max(600.0);

    let mut displays = [0u32; 16];
    let mut count = 0u32;
    let err = unsafe { CGGetActiveDisplayList(16, displays.as_mut_ptr(), &mut count) };
    let display_slice = if err == 0 && count > 0 {
        &displays[..(count as usize).min(16)]
    } else {
        std::slice::from_ref(&main_id)
    };

    let all_bounds: Vec<Rect> = display_slice
        .iter()
        .map(|&id| {
            let b = unsafe { CGDisplayBounds(id) };
            Rect {
                x: b.origin.x,
                y: b.origin.y,
                width: b.size.width.max(800.0),
                height: b.size.height.max(600.0),
            }
        })
        .collect();

    if let Some(target) = target_rect {
        let cx = target.x + target.width * 0.5;
        let cy = target.y + target.height * 0.5;
        if let Some(found) = all_bounds
            .iter()
            .copied()
            .find(|b| cx >= b.x && cx <= b.x + b.width && cy >= b.y && cy <= b.y + b.height)
        {
            return (found.x, found.y, found.width, found.height, main_h);
        }
        // Fallback to largest intersection area with target_rect
        let mut best: Option<(f64, Rect)> = None;
        for b in &all_bounds {
            if let Some(inter) = target.intersection(*b) {
                let area = inter.width * inter.height;
                if best.is_none_or(|(best_area, _)| area > best_area) {
                    best = Some((area, *b));
                }
            }
        }
        if let Some((_, b)) = best {
            return (b.x, b.y, b.width, b.height, main_h);
        }
    }

    if let Some(cursor) = fallback_cursor {
        let cx = f64::from(cursor.x);
        let cy = f64::from(cursor.y);
        if let Some(found) = all_bounds
            .iter()
            .copied()
            .find(|b| cx >= b.x && cx <= b.x + b.width && cy >= b.y && cy <= b.y + b.height)
        {
            return (found.x, found.y, found.width, found.height, main_h);
        }
    }

    (
        main_bounds.origin.x,
        main_bounds.origin.y,
        main_bounds.size.width.max(800.0),
        main_bounds.size.height.max(600.0),
        main_h,
    )
}

#[cfg(not(target_os = "macos"))]
fn active_display_quartz_bounds(
    app: &AppHandle,
    _target_rect: Option<Rect>,
    _fallback_cursor: Option<egui::Pos2>,
) -> (f64, f64, f64, f64, f64) {
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor().max(1.0);
        let pos = monitor.position();
        let size = monitor.size();
        let w = (f64::from(size.width) / scale).max(800.0);
        let h = (f64::from(size.height) / scale).max(600.0);
        (f64::from(pos.x) / scale, f64::from(pos.y) / scale, w, h, h)
    } else {
        (0.0, 0.0, 1728.0, 1117.0, 1117.0)
    }
}

#[cfg(target_os = "macos")]
fn is_left_mouse_button_down() -> bool {
    // kCGEventSourceStateCombinedSessionState = 0, kCGMouseButtonLeft = 0
    unsafe { CGEventSourceButtonState(0, 0) }
}

#[cfg(not(target_os = "macos"))]
fn is_left_mouse_button_down() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn install_nonactivating_panel_hook(ns_window: &objc2_app_kit::NSWindow) {
    use objc2::{msg_send, sel};
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| unsafe {
        let ns_win_cls = objc_getClass(c"NSWindow".as_ptr());
        if !ns_win_cls.is_null() {
            let _ = class_replaceMethod(
                ns_win_cls,
                sel!(_isNonactivatingPanel),
                nswindow_is_nonactivating_panel,
                c"B@:".as_ptr(),
            );
        }
        let tao_cls: *mut std::ffi::c_void = msg_send![ns_window, class];
        if !tao_cls.is_null() {
            let _ = class_replaceMethod(
                tao_cls,
                sel!(_isNonactivatingPanel),
                nswindow_is_nonactivating_panel,
                c"B@:".as_ptr(),
            );
            let _ = class_replaceMethod(
                tao_cls,
                sel!(canBecomeKeyWindow),
                nswindow_can_become_key,
                c"B@:".as_ptr(),
            );
            let _ = class_replaceMethod(
                tao_cls,
                sel!(canBecomeMainWindow),
                nswindow_can_become_main,
                c"B@:".as_ptr(),
            );
        }
    });
}

#[cfg(target_os = "macos")]
fn configure_macos_overlay_nswindow(
    app: &AppHandle,
    window: &WebviewWindow,
    mon_x: f64,
    mon_y: f64,
    mon_w: f64,
    mon_h: f64,
    main_h: f64,
) {
    let win = window.clone();
    let _ = app.run_on_main_thread(move || {
        use objc2::rc::Retained;
        use objc2::{msg_send, sel};
        use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
        use objc2_foundation::{NSPoint, NSRect, NSSize};

        let Ok(ns_win_ptr) = win.ns_window() else {
            return;
        };
        let Some(ns_window) = (unsafe { Retained::retain(ns_win_ptr as *mut NSWindow) }) else {
            return;
        };

        install_nonactivating_panel_hook(&ns_window);

        // Prevent clicking on the overlay from activating Elocutio or raising the Studio window
        // in front of the target app.
        unsafe {
            let responds: bool =
                msg_send![&*ns_window, respondsToSelector: sel!(_setPreventsActivation:)];
            if responds {
                let _: () = msg_send![&*ns_window, _setPreventsActivation: true];
            }
        }

        // NSStatusWindowLevel (25) floats above normal application windows and full-screen spaces
        // without stealing key focus. Setting level=25 before setCollectionBehavior causes
        // _isNonactivatingPanel to return YES so WindowServer joins full-screen spaces.
        ns_window.setLevel(25);
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        ns_window.setOpaque(false);
        ns_window.setHasShadow(false);
        ns_window.setMovable(false);

        let appkit_x = mon_x;
        let appkit_y = main_h - (mon_y + mon_h);
        let frame = NSRect::new(NSPoint::new(appkit_x, appkit_y), NSSize::new(mon_w, mon_h));
        ns_window.setFrame_display(frame, true);
        ns_window.orderFrontRegardless();
    });
}

#[cfg(target_os = "macos")]
fn sync_overlay_window_to_display(
    app: &AppHandle,
    mon_x: f64,
    mon_y: f64,
    mon_w: f64,
    mon_h: f64,
    main_h: f64,
    force_order_front: bool,
) {
    let mut needs_update = force_order_front;
    if let Ok(mut guard) = shared_state().lock() {
        let changed = (guard.monitor_origin.0 - mon_x).abs() > 0.5
            || (guard.monitor_origin.1 - mon_y).abs() > 0.5
            || (guard.monitor_size.0 - mon_w).abs() > 0.5
            || (guard.monitor_size.1 - mon_h).abs() > 0.5
            || (guard.main_display_height - main_h).abs() > 0.5;
        if changed {
            guard.monitor_origin = (mon_x, mon_y);
            guard.monitor_size = (mon_w, mon_h);
            guard.main_display_height = main_h;
            needs_update = true;
            eprintln!(
                "elo overlay display synced: quartz=({mon_x}, {mon_y}, {mon_w}x{mon_h}) main_h={main_h}"
            );
        }
    }

    if !needs_update {
        return;
    }

    let Some(win) = app.get_webview_window("elo-overlay") else {
        return;
    };
    let _ = app.run_on_main_thread(move || {
        use objc2::rc::Retained;
        use objc2_app_kit::NSWindow;
        use objc2_foundation::{NSPoint, NSRect, NSSize};

        let Ok(ns_win_ptr) = win.ns_window() else {
            return;
        };
        let Some(ns_window) = (unsafe { Retained::retain(ns_win_ptr as *mut NSWindow) }) else {
            return;
        };

        let appkit_x = mon_x;
        let appkit_y = main_h - (mon_y + mon_h);
        let frame = NSRect::new(NSPoint::new(appkit_x, appkit_y), NSSize::new(mon_w, mon_h));
        let cur = ns_window.frame();
        if (cur.origin.x - frame.origin.x).abs() > 0.5
            || (cur.origin.y - frame.origin.y).abs() > 0.5
            || (cur.size.width - frame.size.width).abs() > 0.5
            || (cur.size.height - frame.size.height).abs() > 0.5
        {
            ns_window.setFrame_display(frame, true);
        }
        ns_window.setLevel(25);
        ns_window.orderFrontRegardless();
    });
}

#[cfg(not(target_os = "macos"))]
fn sync_overlay_window_to_display(
    _app: &AppHandle,
    mon_x: f64,
    mon_y: f64,
    mon_w: f64,
    mon_h: f64,
    main_h: f64,
    _force_order_front: bool,
) {
    if let Ok(mut guard) = shared_state().lock() {
        guard.monitor_origin = (mon_x, mon_y);
        guard.monitor_size = (mon_w, mon_h);
        guard.main_display_height = main_h;
    }
}

fn set_window_ignore_cursor(app: &AppHandle, ignore: bool) {
    let prev = IGNORING_CURSOR.swap(ignore, Ordering::Relaxed);
    if prev != ignore
        && let Some(window) = app.get_webview_window("elo-overlay")
    {
        let _ = window.set_ignore_cursor_events(ignore);
    }
}

fn compute_widget_rect(
    field_rect: EloRect,
    offset_x: f64,
    offset_y: f64,
    mon_w: f64,
    mon_h: f64,
) -> EloRect {
    let w = 104.0;
    let h = 38.0;
    let min_x = (field_rect.x + 6.0).clamp(8.0, (mon_w - w - 8.0).max(8.0));
    let max_x = (field_rect.x + field_rect.width - w - 8.0)
        .max(min_x)
        .min((mon_w - w - 8.0).max(8.0));
    let base_x = max_x;

    let (min_y, max_y, base_y) = if field_rect.height < 56.0 {
        let by = (field_rect.y + field_rect.height + 4.0).clamp(32.0, (mon_h - h - 8.0).max(32.0));
        (
            (field_rect.y - h - 4.0).max(32.0),
            (field_rect.y + field_rect.height + 12.0).min((mon_h - h - 8.0).max(32.0)),
            by,
        )
    } else {
        let top_bound = (field_rect.y + 6.0).clamp(32.0, (mon_h - h - 8.0).max(32.0));
        let bottom_bound = (field_rect.y + field_rect.height - h - 8.0)
            .max(top_bound)
            .min((mon_h - h - 8.0).max(32.0));
        (top_bound, bottom_bound, bottom_bound)
    };

    let x = (base_x + offset_x).clamp(min_x, max_x);
    let y = (base_y + offset_y).clamp(min_y, max_y);
    EloRect {
        x,
        y,
        width: w,
        height: h,
    }
}

fn categorize_lint_kind(
    kind: LintKind,
) -> (&'static str, &'static str, &'static str, &'static str) {
    match kind {
        LintKind::Spelling | LintKind::Typo => {
            ("correctness", "Correctness", "Spelling", "#E5484D")
        }
        LintKind::Punctuation => ("correctness", "Correctness", "Punctuation", "#E5484D"),
        LintKind::Capitalization => ("correctness", "Correctness", "Capitalization", "#E5484D"),
        LintKind::Grammar
        | LintKind::Agreement
        | LintKind::BoundaryError
        | LintKind::Eggcorn
        | LintKind::Malapropism
        | LintKind::Nonstandard
        | LintKind::WordOrder => ("correctness", "Correctness", "Grammar", "#E5484D"),
        LintKind::Redundancy | LintKind::Repetition => {
            ("clarity", "Clarity", "Conciseness", "#2563EB")
        }
        LintKind::Readability => ("clarity", "Clarity", "Readability", "#2563EB"),
        LintKind::WordChoice | LintKind::Enhancement => {
            ("engagement", "Engagement", "Vocabulary", "#10B981")
        }
        LintKind::Usage | LintKind::Regionalism => {
            ("delivery", "Delivery", "Conventions", "#7C3AED")
        }
        LintKind::Style | LintKind::Formatting | LintKind::Miscellaneous => {
            ("style", "Style guide", "Style", "#191F70")
        }
    }
}

fn extract_context_snippets(chars: &[char], start: usize, end: usize) -> (String, String) {
    let len = chars.len();
    let safe_start = start.min(len);
    let safe_end = end.min(len).max(safe_start);

    // Find sentence/line start up to 48 chars before safe_start
    let min_before = safe_start.saturating_sub(48);
    let mut before_idx = safe_start;
    while before_idx > min_before {
        let ch = chars[before_idx - 1];
        if ch == '\n' || ch == '.' || ch == '!' || ch == '?' {
            break;
        }
        before_idx -= 1;
    }
    let mut before: String = chars[before_idx..safe_start].iter().collect();
    if before_idx > 0
        && !matches!(
            chars.get(before_idx.saturating_sub(1)),
            Some('\n' | '.' | '!' | '?')
        )
    {
        before = format!("…{}", before.trim_start());
    }

    // Find sentence/line end up to 48 chars after safe_end
    let max_after = (safe_end + 48).min(len);
    let mut after_idx = safe_end;
    while after_idx < max_after {
        let ch = chars[after_idx];
        if ch == '\n' {
            break;
        }
        after_idx += 1;
        if ch == '.' || ch == '!' || ch == '?' {
            break;
        }
    }
    let mut after: String = chars[safe_end..after_idx].iter().collect();
    if after_idx < len
        && !matches!(
            chars.get(after_idx.saturating_sub(1)),
            Some('.' | '!' | '?' | '\n')
        )
    {
        after = format!("{}…", after.trim_end());
    }

    (before, after)
}

fn category_score(word_count: usize, count: usize, weight: f64) -> f64 {
    if word_count == 0 || count == 0 {
        return 100.0;
    }
    let effective_words = (word_count.max(25)) as f64;
    let rate = (count as f64 / effective_words) * 50.0;
    let exponent = -0.18 * weight * (rate + 0.6 * (count as f64).powf(0.7));
    (100.0 * exponent.exp()).clamp(15.0, 99.0)
}

fn calculate_writing_score(word_count: usize, alerts: &[EloAlert]) -> u32 {
    if word_count == 0 || alerts.is_empty() {
        return 100;
    }

    let mut correctness = 0usize;
    let mut clarity = 0usize;
    let mut engagement = 0usize;
    let mut delivery = 0usize;
    let mut style = 0usize;

    for alert in alerts {
        match alert.category.as_str() {
            "correctness" => correctness += 1,
            "clarity" => clarity += 1,
            "engagement" => engagement += 1,
            "delivery" => delivery += 1,
            _ => style += 1,
        }
    }

    let s_corr = category_score(word_count, correctness, 1.0);
    let s_clar = category_score(word_count, clarity, 0.75);
    let s_eng = category_score(word_count, engagement, 0.60);
    let s_del = category_score(word_count, delivery, 0.65);
    let s_sty = category_score(word_count, style, 0.50);

    let weighted_cat_avg =
        0.38 * s_corr + 0.22 * s_clar + 0.15 * s_eng + 0.15 * s_del + 0.10 * s_sty;

    let weighted_issues = (correctness as f64) * 1.0
        + (clarity as f64) * 0.75
        + (engagement as f64) * 0.60
        + (delivery as f64) * 0.65
        + (style as f64) * 0.50;
    let effective_words = (word_count.max(25)) as f64;
    let overall_rate = (weighted_issues / effective_words) * 50.0;
    let overall_decay = 100.0 * (-0.11 * (overall_rate + 0.55 * weighted_issues.powf(0.75))).exp();

    let combined = (0.45 * weighted_cat_avg + 0.55 * overall_decay).round() as u32;
    combined.clamp(18, 99)
}

fn run_overlay_loop(
    app: AppHandle,
    config: Arc<AsyncMutex<Config>>,
    suggestion_count: Arc<crate::config::SuggestionCount>,
    command_rx: mpsc::Receiver<OverlayCommand>,
) {
    let config_for_policy = config.clone();
    let mut broker = crate::platform_broker(move |bundle_id| {
        let Ok(mut cfg) = config_for_policy.try_lock() else {
            return true;
        };
        cfg.resolve_integration(bundle_id)
    });

    let mut active_lints: Vec<(String, ActionableLint, Vec<Suggestion>)> = Vec::new();
    let mut last_poll = Instant::now() - Duration::from_secs(1);
    let mut last_emitted_hash: u64 = 0;
    let mut prev_mouse_down = false;
    let mut press_origin: Option<(f64, f64)> = None;
    let mut cached_linter_state: Option<(
        harper_core::Dialect,
        usize,
        usize,
        harper_core::linting::LintGroup,
        Arc<harper_core::spell::MergedDictionary>,
    )> = None;
    let mut display_name_cache: HashMap<String, String> = HashMap::new();

    loop {
        thread::sleep(Duration::from_millis(25));

        let mut force_refresh = false;

        // Process any queued commands (ApplyAlert, DismissAlert, RefreshNow)
        while let Ok(cmd) = command_rx.try_recv() {
            match cmd {
                OverlayCommand::ApplyAlert {
                    alert_id,
                    suggestion_index,
                } => {
                    if let Some(pos) = active_lints.iter().position(|(id, _, _)| *id == alert_id) {
                        let (_, mut actionable, suggestions) = active_lints.remove(pos);
                        let chars: Vec<char> = actionable.source_text.chars().collect();
                        let original = actionable.lint.get_str(&chars);
                        let category = actionable.lint.lint_kind.to_string();
                        if let Some(suggestion) = suggestions
                            .get(suggestion_index)
                            .cloned()
                            .or_else(|| suggestions.first().cloned())
                        {
                            let replacement = match &suggestion {
                                Suggestion::ReplaceWith(r) => r.iter().collect::<String>(),
                                Suggestion::InsertAfter(s) => {
                                    format!("{original}{}", s.iter().collect::<String>())
                                }
                                Suggestion::Remove => String::new(),
                            };
                            let _ =
                                crate::voice::record_accepted(&original, &replacement, &category);
                            let _ = crate::voice::refresh_feedback();
                            actionable.apply_suggestion(suggestion);
                        }
                        if let Ok(cfg) = config.try_lock() {
                            let _ = crate::personalization_db::sync_and_status(&cfg);
                        }
                        if let Ok(mut guard) = shared_state().lock() {
                            guard.snapshot.alerts.retain(|a| a.id != alert_id);
                            if guard.snapshot.alerts.is_empty() {
                                guard.frontend_card_rect = None;
                            }
                        }
                        force_refresh = true;
                    }
                }
                OverlayCommand::DismissAlert { alert_id } => {
                    if let Some(pos) = active_lints.iter().position(|(id, _, _)| *id == alert_id) {
                        let (_, actionable, suggestions) = active_lints.remove(pos);
                        let chars: Vec<char> = actionable.source_text.chars().collect();
                        let original = actionable.lint.get_str(&chars);
                        let category = actionable.lint.lint_kind.to_string();
                        let first_repl = suggestions
                            .first()
                            .map(|s| match s {
                                Suggestion::ReplaceWith(r) => r.iter().collect::<String>(),
                                Suggestion::InsertAfter(suf) => {
                                    format!("{original}{}", suf.iter().collect::<String>())
                                }
                                Suggestion::Remove => String::new(),
                            })
                            .unwrap_or_default();
                        let _ = crate::voice::record_dismissed(&original, &first_repl, &category);
                        let _ = crate::voice::refresh_feedback();
                        if let Ok(mut cfg) = config.try_lock() {
                            let dict = Config::dictionary_from_user_dictionary(
                                cfg.mutable_dictionary.clone(),
                            );
                            let doc =
                                Document::new_markdown_default(&actionable.source_text, &dict);
                            cfg.ignored_lints.ignore_lint(&actionable.lint, &doc);
                            let config_arc = config.clone();
                            tauri::async_runtime::spawn(async move {
                                let cfg = config_arc.lock().await;
                                let _ = cfg.save_to_system().await;
                            });
                        }
                        if let Ok(mut guard) = shared_state().lock() {
                            guard.snapshot.alerts.retain(|a| a.id != alert_id);
                            if guard.snapshot.alerts.is_empty() {
                                guard.frontend_card_rect = None;
                            }
                        }
                        force_refresh = true;
                    }
                }
                OverlayCommand::ReplaceFieldText { new_text } => {
                    #[cfg(target_os = "macos")]
                    {
                        let _ = broker.replace_target_text(&new_text);
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        let _ = new_text;
                    }
                    if let Ok(mut guard) = shared_state().lock() {
                        guard.snapshot.source_text = new_text;
                        guard.snapshot.alerts.clear();
                    }
                    force_refresh = true;
                }
                OverlayCommand::RefreshNow => {
                    force_refresh = true;
                }
            }
        }

        // 1. Fast 25ms cursor hit-test so the overlay window is click-through everywhere
        // except over the `"elo"` widget, underlined words, or the open suggestion card.
        let cursor_pos = broker.cursor_position();
        let mut cursor_over_interactive = false;
        let mut logical_cursor: Option<(f64, f64)> = None;
        if let Ok(guard) = shared_state().lock() {
            if !guard.snapshot.service_enabled || !guard.snapshot.field_active {
                cursor_over_interactive = false;
            } else if guard.is_dragging {
                cursor_over_interactive = true;
                if let Some(cursor) = cursor_pos {
                    logical_cursor = Some((
                        f64::from(cursor.x) - guard.monitor_origin.0,
                        f64::from(cursor.y) - guard.monitor_origin.1,
                    ));
                }
            } else if let Some(cursor) = cursor_pos {
                let lx = f64::from(cursor.x) - guard.monitor_origin.0;
                let ly = f64::from(cursor.y) - guard.monitor_origin.1;
                logical_cursor = Some((lx, ly));

                let widget_rect = guard.frontend_widget_rect.or(guard.snapshot.widget_rect);
                if let Some(wr) = widget_rect
                    && wr.contains(lx, ly, 10.0)
                {
                    cursor_over_interactive = true;
                }
                if !cursor_over_interactive
                    && let Some(cr) = guard.frontend_card_rect
                    && cr.contains(lx, ly, 14.0)
                {
                    cursor_over_interactive = true;
                }
                if !cursor_over_interactive {
                    cursor_over_interactive = guard
                        .snapshot
                        .alerts
                        .iter()
                        .any(|a| a.rect.contains(lx, ly, 6.0));
                }
            }
        }
        set_window_ignore_cursor(&app, !cursor_over_interactive);

        let mouse_down = is_left_mouse_button_down();
        if cursor_over_interactive && let Some((lx, ly)) = logical_cursor {
            if mouse_down && !prev_mouse_down {
                press_origin = Some((lx, ly));
            } else if !mouse_down
                && prev_mouse_down
                && let Some((px, py)) = press_origin.take()
                && (lx - px).hypot(ly - py) <= 10.0
            {
                let _ = app.emit("elo-overlay-click", serde_json::json!({ "x": lx, "y": ly }));
            }
        } else {
            if mouse_down && !prev_mouse_down {
                let mut had_card = false;
                if let Ok(mut guard) = shared_state().lock()
                    && guard.frontend_card_rect.is_some()
                {
                    guard.frontend_card_rect = None;
                    had_card = true;
                }
                if OVERLAY_INPUT_FOCUSED.load(Ordering::Relaxed) {
                    set_overlay_input_focus(false);
                }
                if had_card {
                    let _ = app.emit("elo-overlay-outside-click", ());
                    force_refresh = true;
                }
            }
            if !mouse_down {
                press_origin = None;
            }
        }
        prev_mouse_down = mouse_down;

        // 2. Poll macOS Accessibility every 180ms (or immediately on command)
        if !force_refresh && last_poll.elapsed() < Duration::from_millis(180) {
            continue;
        }
        last_poll = Instant::now();

        // While the user is dragging `"elo"`, hovering/clicking `"elo"`, or reviewing an open
        // suggestion/rewrite card (and hasn't focused the Studio window), keep the snapshot stable
        // so clicking on the overlay never loses the target text field or closes the card.
        if !force_refresh
            && !is_studio_window_focused()
            && let Ok(guard) = shared_state().lock()
            && guard.snapshot.field_active
            && (guard.is_dragging
                || cursor_over_interactive
                || guard.frontend_card_rect.is_some()
                || OVERLAY_INPUT_FOCUSED.load(Ordering::Relaxed))
        {
            continue;
        }

        let service_enabled = OVERLAY_RUNNING.load(Ordering::Relaxed);
        let ax_trusted =
            broker.accessibility_permission_status() == AccessibilityPermissionStatus::Granted;

        if !service_enabled || !ax_trusted {
            active_lints.clear();
            suggestion_count.set(0);
            if let Ok(mut guard) = shared_state().lock() {
                guard.snapshot.service_enabled = service_enabled;
                guard.snapshot.ax_trusted = ax_trusted;
                guard.snapshot.field_active = false;
                guard.snapshot.alerts.clear();
                guard.snapshot.widget_rect = None;
                guard.snapshot.field_rect = None;
            }
            set_window_ignore_cursor(&app, true);
            continue;
        }

        // Snapshot config for linting (reusing the compiled LintGroup unless dictionary/dialect changed)
        let (ignored_lints, card_cmd) = {
            use harper_core::spell::Dictionary;
            let Ok(mut cfg) = config.try_lock() else {
                continue;
            };
            let cmd = cfg.card_command.take();
            let dict_len = cfg.mutable_dictionary.word_count();
            let dialect = cfg.dialect;
            let rule_epoch = crate::commands::LINTER_RULE_EPOCH.load(Ordering::Relaxed);
            let needs_rebuild = cached_linter_state
                .as_ref()
                .is_none_or(|(d, len, ep, _, _)| {
                    *d != dialect || *len != dict_len || *ep != rule_epoch
                });
            if needs_rebuild {
                let dict = Config::dictionary_from_user_dictionary(cfg.mutable_dictionary.clone());
                let linter = cfg.create_linter();
                cached_linter_state = Some((dialect, dict_len, rule_epoch, linter, dict));
            }
            (cfg.ignored_lints.clone(), cmd)
        };

        let Some((_, _, _, ref mut linter, ref dictionary)) = cached_linter_state else {
            continue;
        };
        let mut lint_callback = |text: &str| {
            let doc = Document::new_markdown_default(text, dictionary);
            let mut organized = linter.organized_lints(&doc);
            for lints in organized.values_mut() {
                ignored_lints.remove_ignored(lints, &doc);
            }
            organized
        };

        let boxes = broker.get_boxes(&mut lint_callback);
        let has_active_field = broker.has_active_field();
        let raw_field_rect = broker.active_field_rect();
        let tone = broker.detected_tone();
        let bundle_id = broker
            .focused_bundle_id()
            .ok()
            .flatten()
            .unwrap_or_default();
        let app_name = if bundle_id.is_empty() {
            String::new()
        } else if let Some(cached_name) = display_name_cache.get(&bundle_id) {
            cached_name.clone()
        } else {
            let name = broker.integration_display_name(&bundle_id);
            display_name_cache.insert(bundle_id.clone(), name.clone());
            name
        };

        let (mon_x, mon_y, mon_w, mon_h, main_h) =
            active_display_quartz_bounds(&app, raw_field_rect, cursor_pos);
        let app_changed = shared_state()
            .lock()
            .map(|g| g.snapshot.app_bundle_id != bundle_id && has_active_field)
            .unwrap_or(false);
        sync_overlay_window_to_display(&app, mon_x, mon_y, mon_w, mon_h, main_h, app_changed);

        let mut new_active_lints: Vec<(String, ActionableLint, Vec<Suggestion>)> = Vec::new();
        let mut alerts: Vec<EloAlert> = Vec::new();
        let mut source_text = String::new();

        if let Some(items) = boxes {
            for actionable in items {
                if source_text.is_empty() && !actionable.source_text.is_empty() {
                    source_text = actionable.source_text.clone();
                }
                let chars: Vec<char> = actionable.source_text.chars().collect();
                if actionable.lint.span.is_empty() || actionable.lint.span.end > chars.len() {
                    continue;
                }
                let original = actionable.lint.get_str(&chars);
                let (category, category_label, subcategory, color) =
                    categorize_lint_kind(actionable.lint.lint_kind);
                let (context_before, context_after) = extract_context_snippets(
                    &chars,
                    actionable.lint.span.start,
                    actionable.lint.span.end,
                );

                let raw_suggestions = actionable.lint.suggestions.clone();
                let suggestions: Vec<EloSuggestion> = raw_suggestions
                    .iter()
                    .map(|s| match s {
                        Suggestion::ReplaceWith(repl_chars) => {
                            let repl: String = repl_chars.iter().collect();
                            EloSuggestion {
                                kind: "replace".to_string(),
                                label: if repl.is_empty() {
                                    "Remove".to_string()
                                } else {
                                    repl.clone()
                                },
                                replacement: repl,
                            }
                        }
                        Suggestion::InsertAfter(suffix_chars) => {
                            let suffix: String = suffix_chars.iter().collect();
                            let combined = format!("{original}{suffix}");
                            EloSuggestion {
                                kind: "insert_after".to_string(),
                                label: combined.clone(),
                                replacement: combined,
                            }
                        }
                        Suggestion::Remove => EloSuggestion {
                            kind: "remove".to_string(),
                            label: "Remove".to_string(),
                            replacement: String::new(),
                        },
                    })
                    .collect();

                let id = format!(
                    "{}:{}:{}",
                    actionable.rule_name, actionable.lint.span.start, actionable.lint.span.end
                );

                // Avoid overlapping duplicate spans
                if alerts.iter().any(|existing: &EloAlert| {
                    actionable.lint.span.start < existing.char_end
                        && existing.char_start < actionable.lint.span.end
                }) {
                    continue;
                }

                let rect = EloRect::from_rect(actionable.rect, mon_x, mon_y);
                alerts.push(EloAlert {
                    id: id.clone(),
                    rule_name: actionable.rule_name.clone(),
                    category: category.to_string(),
                    category_label: category_label.to_string(),
                    subcategory: subcategory.to_string(),
                    color: color.to_string(),
                    original,
                    message: actionable.lint.message.clone(),
                    context_before,
                    context_after,
                    suggestions,
                    char_start: actionable.lint.span.start,
                    char_end: actionable.lint.span.end,
                    rect,
                });

                new_active_lints.push((id, actionable, raw_suggestions));
            }
        }

        #[cfg(target_os = "macos")]
        if source_text.is_empty()
            && has_active_field
            && let Some(text) = broker.active_field_text()
        {
            source_text = text;
        }
        #[cfg(not(target_os = "macos"))]
        if source_text.is_empty()
            && has_active_field
            && let Ok(Some(ft)) = broker.focused_text()
        {
            source_text = ft.text;
        }

        let word_count = source_text.split_whitespace().count();
        let score = calculate_writing_score(word_count, &alerts);
        suggestion_count.set(alerts.len());
        active_lints = new_active_lints;

        let field_rect = raw_field_rect.map(|r| EloRect::from_rect(r, mon_x, mon_y));
        #[cfg(target_os = "macos")]
        let occluders: Vec<EloRect> = broker
            .occluding_rects()
            .iter()
            .map(|r| EloRect::from_rect(*r, mon_x, mon_y))
            .collect();
        #[cfg(not(target_os = "macos"))]
        let occluders: Vec<EloRect> = Vec::new();
        let card_command_str = card_cmd.map(|c| match c {
            crate::config::CardCommand::Focus => "focus".to_string(),
            crate::config::CardCommand::NextSuggestion => "next".to_string(),
            crate::config::CardCommand::PreviousSuggestion => "prev".to_string(),
            crate::config::CardCommand::NextReplacement => "next_repl".to_string(),
            crate::config::CardCommand::PreviousReplacement => "prev_repl".to_string(),
            crate::config::CardCommand::Accept => "accept".to_string(),
            crate::config::CardCommand::Dismiss => "dismiss".to_string(),
            crate::config::CardCommand::Close => "close".to_string(),
        });

        #[cfg(target_os = "macos")]
        let target_pid = broker.focused_pid().unwrap_or(0);
        #[cfg(not(target_os = "macos"))]
        let target_pid = 0;

        let snapshot = if let Ok(mut guard) = shared_state().lock() {
            let offset_key = if bundle_id.is_empty() {
                "default"
            } else {
                bundle_id.as_str()
            };
            let (ox, oy) = guard
                .widget_offsets
                .get(offset_key)
                .copied()
                .unwrap_or((0.0, 0.0));
            let widget_rect = if has_active_field {
                field_rect.and_then(|fr| {
                    let mut wr = compute_widget_rect(fr, ox, oy, mon_w, mon_h);
                    // If an occluding window covers the right side of the field where "elo" sits,
                    // shift "elo" left into the visible portion of the field, or hide it if covered.
                    for occ in &occluders {
                        let overlaps = wr.x < occ.x + occ.width
                            && wr.x + wr.width > occ.x
                            && wr.y < occ.y + occ.height
                            && wr.y + wr.height > occ.y;
                        if overlaps {
                            if occ.x > fr.x + wr.width + 16.0 {
                                wr.x = (occ.x - wr.width - 10.0).max(fr.x + 6.0);
                            } else {
                                return None;
                            }
                        }
                    }
                    let still_occluded = occluders.iter().any(|occ| {
                        wr.x < occ.x + occ.width
                            && wr.x + wr.width > occ.x
                            && wr.y < occ.y + occ.height
                            && wr.y + wr.height > occ.y
                    });
                    (!still_occluded).then_some(wr)
                })
            } else {
                None
            };

            let snap = EloOverlaySnapshot {
                service_enabled,
                ax_trusted,
                field_active: has_active_field && field_rect.is_some(),
                pid: target_pid,
                app_bundle_id: bundle_id,
                app_name,
                field_rect,
                widget_rect,
                alerts,
                tone,
                score,
                word_count,
                source_text,
                card_command: card_command_str,
            };
            guard.snapshot = snap.clone();
            snap
        } else {
            continue;
        };

        // Emit update event when snapshot changes
        let state_hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            snapshot.field_active.hash(&mut hasher);
            snapshot.app_bundle_id.hash(&mut hasher);
            (mon_x as i32).hash(&mut hasher);
            (mon_y as i32).hash(&mut hasher);
            (mon_w as i32).hash(&mut hasher);
            (mon_h as i32).hash(&mut hasher);
            snapshot.alerts.len().hash(&mut hasher);
            for a in &snapshot.alerts {
                a.id.hash(&mut hasher);
                a.message.hash(&mut hasher);
                a.category.hash(&mut hasher);
                (a.rect.x as i32).hash(&mut hasher);
                (a.rect.y as i32).hash(&mut hasher);
            }
            if let Some(fr) = snapshot.field_rect {
                (fr.x as i32).hash(&mut hasher);
                (fr.y as i32).hash(&mut hasher);
                (fr.width as i32).hash(&mut hasher);
                (fr.height as i32).hash(&mut hasher);
            }
            if let Some(wr) = snapshot.widget_rect {
                (wr.x as i32).hash(&mut hasher);
                (wr.y as i32).hash(&mut hasher);
            }
            snapshot.card_command.hash(&mut hasher);
            hasher.finish()
        };

        if state_hash != last_emitted_hash || snapshot.card_command.is_some() {
            last_emitted_hash = state_hash;
            let _ = app.emit("elo-overlay-snapshot", &snapshot);
        }
    }
}

/// Resets stale macOS TCC Accessibility entry for `app.elocutio.desktop` when untrusted
/// and triggers the system Accessibility prompt + opens System Settings -> Accessibility.
pub fn reset_and_request_accessibility(
    broker: &std::sync::RwLock<crate::PlatformBroker>,
) -> AccessibilityPermissionStatus {
    let current = broker
        .read()
        .map(|b| b.accessibility_permission_status())
        .unwrap_or(AccessibilityPermissionStatus::NotGranted);

    if current == AccessibilityPermissionStatus::Granted {
        return AccessibilityPermissionStatus::Granted;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("tccutil")
            .args(["reset", "Accessibility", "app.elocutio.desktop"])
            .status();
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }

    let status = broker
        .read()
        .map(|b| b.request_accessibility_permission())
        .unwrap_or(AccessibilityPermissionStatus::NotGranted);

    request_refresh();
    status
}
