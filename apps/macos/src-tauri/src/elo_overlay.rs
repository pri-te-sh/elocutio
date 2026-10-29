//! Single-process system-wide `"elo"` floating widget & inline underline overlay.
//!
//! Replaces the legacy child-process `winit`/`egui`/`wgpu` overlay with a transparent,
//! non-activating, always-on-top Tauri `WebviewWindow` (`label = "elo-overlay"`) backed by
//! two supervised threads: one polls macOS Accessibility, the other hit-tests the cursor so the
//! overlay's click-through state never waits on a slow target app.

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{
    Arc, Mutex as StdMutex, MutexGuard, OnceLock, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{self, Receiver, RecvTimeoutError, Sender},
};
use std::thread;
use std::time::{Duration, Instant};

use harper_core::{
    Document,
    linting::{Lint, LintKind, Suggestion},
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
    /// The whole-sentence correction this alert belongs to, when its sentence has one.
    pub sentence_fix_id: Option<String>,
}

/// The text the user has selected in the active field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EloSelection {
    pub char_start: usize,
    pub char_end: usize,
    pub text: String,
    pub rect: EloRect,
}

/// One run of a sentence correction's diff: text kept, removed, or inserted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EloDiffSegment {
    /// `"equal"`, `"delete"` or `"insert"`.
    pub kind: String,
    pub text: String,
}

/// A correction of a whole sentence, built from every fixable alert inside it.
///
/// Offered when a sentence holds several alerts, so the user can accept the corrected sentence in
/// one step instead of fixing each word in turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EloSentenceFix {
    pub id: String,
    /// Char offset of the sentence in the field's text.
    pub char_start: usize,
    /// Char offset (exclusive) of the sentence in the field's text.
    pub char_end: usize,
    pub original: String,
    pub corrected: String,
    pub alert_ids: Vec<String>,
    pub segments: Vec<EloDiffSegment>,
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
    /// The selection in the active field; the widget only shows while there is one.
    pub selection: Option<EloSelection>,
    /// Whole-sentence corrections for sentences with several alerts.
    pub sentence_fixes: Vec<EloSentenceFix>,
    /// A card shortcut to run, present only on the one snapshot event that delivers it.
    pub card_command: Option<String>,
    /// Incremented for every card command, so the frontend can run each command exactly once
    /// even when it sees the same snapshot again through its poll.
    #[serde(rename = "card_command_seq")]
    pub card_command_seq: u64,
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
    ApplySentenceFix {
        fix_id: String,
    },
    DismissSentenceFix {
        fix_id: String,
    },
    ReplaceSelection {
        char_start: usize,
        char_end: usize,
        original: String,
        new_text: String,
    },
    RefreshNow,
}

struct SharedOverlayState {
    snapshot: EloOverlaySnapshot,
    frontend_widget_rect: Option<EloRect>,
    frontend_card_rect: Option<EloRect>,
    is_dragging: bool,
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
            monitor_origin: (0.0, 0.0),
            monitor_size: (1728.0, 1117.0),
            main_display_height: 1117.0,
            studio_launch: StudioLaunchState::default(),
            command_tx: None,
        }
    }
}

/// What the overlay loop thread needs to be (re)started.
struct LoopContext {
    app: AppHandle,
    config: Arc<AsyncMutex<Config>>,
    suggestion_count: Arc<crate::config::SuggestionCount>,
}

static OVERLAY_STATE: OnceLock<StdMutex<SharedOverlayState>> = OnceLock::new();
static OVERLAY_APP: OnceLock<AppHandle> = OnceLock::new();
static LOOP_CONTEXT: OnceLock<LoopContext> = OnceLock::new();
static OVERLAY_RUNNING: AtomicBool = AtomicBool::new(false);
static IGNORING_CURSOR: AtomicBool = AtomicBool::new(true);
static OVERLAY_INPUT_FOCUSED: AtomicBool = AtomicBool::new(false);
/// Whether the cursor is over the widget, an underline, or the card; written by the hit-test
/// thread and read by the polling loop to keep the snapshot stable while the user interacts.
static CURSOR_OVER_INTERACTIVE: AtomicBool = AtomicBool::new(false);
/// Milliseconds since [`process_epoch`] at the polling loop's last heartbeat; 0 means no live loop.
static LOOP_HEARTBEAT_MS: AtomicU64 = AtomicU64::new(0);
/// The generation of the polling loop that should be running. A loop from an older generation
/// (one the watchdog gave up on as wedged) exits as soon as it wakes.
static LOOP_GENERATION: AtomicU64 = AtomicU64::new(0);

/// How often the polling loop reads the focused field while one is active.
const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(180);
/// How often the polling loop looks for a field while paused or while no field is active.
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// How often the hit-test thread checks the cursor while there is something to hover.
const HIT_TEST_INTERVAL: Duration = Duration::from_millis(25);
/// How often the hit-test thread checks while there is nothing interactive on screen.
const HIT_TEST_IDLE_INTERVAL: Duration = Duration::from_millis(250);
/// A loop that has not heartbeat for this long is considered dead or wedged. Generous, because
/// one poll may legitimately spend a few seconds in slow Accessibility calls.
const LOOP_STALE_AFTER: Duration = Duration::from_secs(20);
/// How many panics a supervised thread survives before it gives up and reports itself dead.
const MAX_THREAD_RESTARTS: u32 = 5;

fn shared_state() -> &'static StdMutex<SharedOverlayState> {
    OVERLAY_STATE.get_or_init(|| StdMutex::new(SharedOverlayState::default()))
}

/// Locks the shared overlay state, recovering it if a panicking thread poisoned the lock.
///
/// The state is plain data that every writer leaves consistent, and a poisoned lock must not
/// take the whole overlay down with it.
fn lock_state() -> MutexGuard<'static, SharedOverlayState> {
    shared_state()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn process_epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Records that the polling loop is alive right now.
fn heartbeat() {
    let elapsed = u64::try_from(process_epoch().elapsed().as_millis()).unwrap_or(u64::MAX);
    LOOP_HEARTBEAT_MS.store(elapsed.max(1), Ordering::Relaxed);
}

/// Whether a heartbeat at `heartbeat_ms` is recent enough at `now_ms` for the loop to count as alive.
fn heartbeat_is_fresh(heartbeat_ms: u64, now_ms: u64) -> bool {
    heartbeat_ms != 0
        && u128::from(now_ms.saturating_sub(heartbeat_ms)) < LOOP_STALE_AFTER.as_millis()
}

/// Whether the in-process `"elo"` overlay service is currently enabled.
pub fn is_running() -> bool {
    OVERLAY_RUNNING.load(Ordering::Relaxed)
}

/// Whether the polling loop thread is alive and has heartbeat recently.
pub fn is_loop_alive() -> bool {
    let now = u64::try_from(process_epoch().elapsed().as_millis()).unwrap_or(u64::MAX);
    heartbeat_is_fresh(LOOP_HEARTBEAT_MS.load(Ordering::Relaxed), now)
}

/// Respawns the polling loop when it is not alive. Requires [`start`] to have run.
pub fn ensure_loop_alive() -> Result<(), String> {
    if is_loop_alive() {
        return Ok(());
    }
    spawn_overlay_loop()
}

/// Stops the polling loop and hands the target app back the accessibility settings Elocutio
/// changed. Called when the app quits.
pub fn shutdown() {
    LOOP_GENERATION.fetch_add(1, Ordering::SeqCst);
    LOOP_HEARTBEAT_MS.store(0, Ordering::Relaxed);
    #[cfg(target_os = "macos")]
    crate::mac_broker::release_accessibility_activation_on_exit();
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
        let target_pid = lock_state().snapshot.pid;
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
    let mut guard = lock_state();
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

/// Returns the latest `"elo"` overlay snapshot.
pub fn current_snapshot() -> EloOverlaySnapshot {
    lock_state().snapshot.clone()
}

/// Takes any pending studio launch state (requested tab or imported text).
pub fn take_studio_launch_state() -> StudioLaunchState {
    std::mem::take(&mut lock_state().studio_launch)
}

/// Sets the studio launch state before opening the main window.
pub fn set_studio_launch_state(tab: Option<String>, import_focused_text: bool) {
    let mut guard = lock_state();
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

/// Updates the interactive hit regions reported by the `"elo-overlay"` frontend.
pub fn update_interactive_regions(
    widget_rect: Option<EloRect>,
    card_rect: Option<EloRect>,
    is_dragging: bool,
) {
    {
        let mut guard = lock_state();
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

/// Sends a command to the overlay loop thread.
fn send_command(command: OverlayCommand) -> Result<(), String> {
    let tx = lock_state()
        .command_tx
        .clone()
        .ok_or_else(|| "Overlay worker is not running.".to_string())?;
    tx.send(command)
        .map_err(|_| "The overlay worker stopped before it received the command.".to_string())
}

/// Sends an ApplyAlert command to the overlay worker thread.
pub fn request_apply_alert(alert_id: String, suggestion_index: usize) -> Result<bool, String> {
    send_command(OverlayCommand::ApplyAlert {
        alert_id,
        suggestion_index,
    })
    .map_err(|e| format!("Failed to send apply command: {e}"))?;
    Ok(true)
}

/// Sends a DismissAlert command to the overlay worker thread.
pub fn request_dismiss_alert(alert_id: String) -> Result<bool, String> {
    send_command(OverlayCommand::DismissAlert { alert_id })
        .map_err(|e| format!("Failed to send dismiss command: {e}"))?;
    Ok(true)
}

/// Sends a ReplaceFieldText command to the overlay worker thread to replace the entire field text.
pub fn request_replace_field_text(new_text: String) -> Result<bool, String> {
    send_command(OverlayCommand::ReplaceFieldText { new_text })
        .map_err(|e| format!("Failed to send replace command: {e}"))?;
    Ok(true)
}

/// Triggers an immediate re-poll on the overlay worker thread.
/// Asks the overlay loop to replace a whole sentence with its correction.
pub fn request_apply_sentence_fix(fix_id: String) -> Result<bool, String> {
    send_command(OverlayCommand::ApplySentenceFix { fix_id })?;
    Ok(true)
}

/// Asks the overlay loop to dismiss every alert in a sentence correction.
pub fn request_dismiss_sentence_fix(fix_id: String) -> Result<bool, String> {
    send_command(OverlayCommand::DismissSentenceFix { fix_id })?;
    Ok(true)
}

/// Asks the overlay loop to replace the selected text (a rewrite of the selection).
pub fn request_replace_selection(
    char_start: usize,
    char_end: usize,
    original: String,
    new_text: String,
) -> Result<bool, String> {
    send_command(OverlayCommand::ReplaceSelection {
        char_start,
        char_end,
        original,
        new_text,
    })?;
    Ok(true)
}

pub fn request_refresh() {
    let _ = send_command(OverlayCommand::RefreshNow);
}

/// Creates the transparent always-on-top `"elo-overlay"` window and starts the background
/// polling and hit-testing threads.
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
    lock_state().snapshot.service_enabled = enabled;

    if let Err(err) = ensure_overlay_window(&app) {
        tracing::error!("Could not create elo-overlay window: {err}");
    }

    let _ = LOOP_CONTEXT.set(LoopContext {
        app: app.clone(),
        config,
        suggestion_count,
    });

    // Hit-testing gets its own thread so the overlay's click-through state never waits on the
    // polling loop, which can stall in Accessibility calls to a slow target app.
    let hit_test_app = app.clone();
    let give_up_app = app.clone();
    if let Err(error) = spawn_supervised(
        "elocutio-elo-overlay-hittest",
        move || run_hit_test_loop(&hit_test_app),
        // Without hit-testing nothing would switch the overlay back to click-through.
        move || set_window_ignore_cursor(&give_up_app, true),
    ) {
        tracing::error!("Could not start the elo-overlay hit-test thread: {error}");
    }

    if let Err(error) = spawn_overlay_loop() {
        tracing::error!("Could not start the elo-overlay loop: {error}");
    }
}

/// Spawns a thread that runs `body` again after a panic, with backoff, up to
/// [`MAX_THREAD_RESTARTS`] times; then it runs `on_give_up` and exits. `body` returning
/// normally ends the thread.
fn spawn_supervised(
    name: &str,
    body: impl Fn() + Send + 'static,
    on_give_up: impl FnOnce() + Send + 'static,
) -> std::io::Result<thread::JoinHandle<()>> {
    let thread_name = name.to_string();
    thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            let mut restarts = 0u32;
            loop {
                match panic::catch_unwind(AssertUnwindSafe(&body)) {
                    Ok(()) => return,
                    Err(payload) => {
                        restarts += 1;
                        let message = payload
                            .downcast_ref::<&str>()
                            .map(|s| (*s).to_string())
                            .or_else(|| payload.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "unknown panic".to_string());
                        if restarts > MAX_THREAD_RESTARTS {
                            tracing::error!(
                                "{thread_name} panicked {restarts} times; giving up: {message}"
                            );
                            on_give_up();
                            return;
                        }
                        tracing::error!(
                            "{thread_name} panicked; restarting (attempt {restarts}): {message}"
                        );
                        thread::sleep(restart_backoff(restarts));
                    }
                }
            }
        })
}

/// Delay before the `restarts`-th restart: 250 ms doubling up to 8 s.
fn restart_backoff(restarts: u32) -> Duration {
    Duration::from_millis(250u64.saturating_mul(1 << restarts.saturating_sub(1).min(5)))
}

/// Starts a new generation of the polling loop, superseding any older loop thread.
fn spawn_overlay_loop() -> Result<(), String> {
    let context = LOOP_CONTEXT
        .get()
        .ok_or_else(|| "The overlay has not been started yet.".to_string())?;
    let generation = LOOP_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    // Replacing the sender disconnects an older loop's receiver, which also makes it exit.
    let (command_tx, command_rx) = mpsc::channel::<OverlayCommand>();
    lock_state().command_tx = Some(command_tx);
    heartbeat();

    let app = context.app.clone();
    let config = context.config.clone();
    let suggestion_count = context.suggestion_count.clone();
    let command_rx = StdMutex::new(command_rx);
    spawn_supervised(
        "elocutio-elo-overlay-loop",
        move || {
            if LOOP_GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            heartbeat();
            let command_rx = command_rx.lock().unwrap_or_else(PoisonError::into_inner);
            run_overlay_loop(&app, &config, &suggestion_count, &command_rx, generation);
        },
        // Report the loop dead at once so the watchdog can start a fresh one.
        move || {
            if LOOP_GENERATION.load(Ordering::SeqCst) == generation {
                LOOP_HEARTBEAT_MS.store(0, Ordering::Relaxed);
            }
        },
    )
    .map_err(|error| {
        LOOP_HEARTBEAT_MS.store(0, Ordering::Relaxed);
        format!("could not spawn the overlay loop thread: {error}")
    })?;
    Ok(())
}

/// Builds and configures the transparent, non-activating, always-on-top `"elo-overlay"` window.
pub fn ensure_overlay_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(existing) = app.get_webview_window("elo-overlay") {
        return Ok(existing);
    }

    let (mon_x, mon_y, mon_w, mon_h, main_h) = active_display_quartz_bounds(app, None, None);
    {
        let mut guard = lock_state();
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
    {
        let mut guard = lock_state();
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
    let mut guard = lock_state();
    guard.monitor_origin = (mon_x, mon_y);
    guard.monitor_size = (mon_w, mon_h);
    guard.main_display_height = main_h;
}

fn set_window_ignore_cursor(app: &AppHandle, ignore: bool) {
    let prev = IGNORING_CURSOR.swap(ignore, Ordering::Relaxed);
    if prev != ignore
        && let Some(window) = app.get_webview_window("elo-overlay")
    {
        let _ = window.set_ignore_cursor_events(ignore);
    }
}

/// The selection widget's size: two icon buttons in a pill.
const PILL_WIDTH: f64 = 92.0;
const PILL_HEIGHT: f64 = 42.0;
/// The gap between the selection and the widget.
const PILL_GAP: f64 = 6.0;

/// Where the widget sits for a selection: just below its bottom-left corner, or above it when
/// there is no room below. Always on screen, whatever the selection geometry.
fn compute_selection_pill_rect(selection: EloRect, mon_w: f64, mon_h: f64) -> EloRect {
    let screen_max_x = (mon_w - PILL_WIDTH - 8.0).max(8.0);
    let screen_max_y = (mon_h - PILL_HEIGHT - 8.0).max(32.0);
    let below = selection.y + selection.height + PILL_GAP;
    let y = if below <= screen_max_y {
        below
    } else {
        selection.y - PILL_HEIGHT - PILL_GAP
    };
    EloRect {
        x: clamp_between(selection.x, 8.0, screen_max_x),
        y: clamp_between(y, 32.0, screen_max_y),
        width: PILL_WIDTH,
        height: PILL_HEIGHT,
    }
}

/// The sentences of `chars` as `(start, end)` char ranges, without surrounding whitespace.
///
/// A sentence ends after `.`, `!` or `?` followed by whitespace (or the end of the text), and at
/// every line break. That is deliberately simple: an abbreviation such as "e.g. this" splits a
/// sentence, which only means a sentence correction is offered for a shorter span.
fn sentence_ranges(chars: &[char]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let push = |start: usize, end: usize, ranges: &mut Vec<(usize, usize)>| {
        let mut s = start;
        let mut e = end;
        while s < e && chars[s].is_whitespace() {
            s += 1;
        }
        while e > s && chars[e - 1].is_whitespace() {
            e -= 1;
        }
        if s < e {
            ranges.push((s, e));
        }
    };
    for (index, &c) in chars.iter().enumerate() {
        let ends_sentence = c == '\n'
            || (matches!(c, '.' | '!' | '?')
                && chars.get(index + 1).is_none_or(|next| next.is_whitespace()));
        if ends_sentence {
            push(start, index + 1, &mut ranges);
            start = index + 1;
        }
    }
    push(start, chars.len(), &mut ranges);
    ranges
}

/// Builds a whole-sentence correction for every sentence that holds at least two alerts with a
/// suggestion, and links those alerts to it.
///
/// The corrected sentence applies each alert's first suggestion. The diff segments come straight
/// from those edits rather than from a text diff, so they show exactly what each fix changes.
fn build_sentence_fixes(source_text: &str, alerts: &mut [EloAlert]) -> Vec<EloSentenceFix> {
    const MIN_ALERTS: usize = 2;
    let chars: Vec<char> = source_text.chars().collect();
    let mut fixes = Vec::new();

    for (start, end) in sentence_ranges(&chars) {
        let mut members: Vec<usize> = alerts
            .iter()
            .enumerate()
            .filter(|(_, alert)| {
                alert.char_start >= start
                    && alert.char_end <= end
                    && alert.char_start < alert.char_end
                    && !alert.suggestions.is_empty()
            })
            .map(|(index, _)| index)
            .collect();
        if members.len() < MIN_ALERTS {
            continue;
        }
        members.sort_by_key(|&index| alerts[index].char_start);

        let mut segments: Vec<EloDiffSegment> = Vec::new();
        let mut push = |kind: &str, text: String| {
            if text.is_empty() {
                return;
            }
            match segments.last_mut() {
                Some(last) if last.kind == kind => last.text.push_str(&text),
                _ => segments.push(EloDiffSegment {
                    kind: kind.to_string(),
                    text,
                }),
            }
        };
        let mut cursor = start;
        let mut overlapping = false;
        for &index in &members {
            let alert = &alerts[index];
            if alert.char_start < cursor {
                overlapping = true;
                break;
            }
            push("equal", chars[cursor..alert.char_start].iter().collect());
            let original: String = chars[alert.char_start..alert.char_end].iter().collect();
            let suggestion = &alert.suggestions[0];
            match suggestion.kind.as_str() {
                // An insertion keeps the flagged text and adds after it.
                "insert_after" => {
                    let added = suggestion
                        .replacement
                        .strip_prefix(original.as_str())
                        .unwrap_or(&suggestion.replacement)
                        .to_string();
                    push("equal", original);
                    push("insert", added);
                }
                _ => {
                    push("delete", original);
                    push("insert", suggestion.replacement.clone());
                }
            }
            cursor = alert.char_end;
        }
        if overlapping {
            continue;
        }
        push("equal", chars[cursor..end].iter().collect());

        let corrected: String = segments
            .iter()
            .filter(|segment| segment.kind != "delete")
            .map(|segment| segment.text.as_str())
            .collect();
        let original: String = chars[start..end].iter().collect();
        if corrected == original {
            continue;
        }
        let id = format!("sentence:{start}:{end}");
        for &index in &members {
            alerts[index].sentence_fix_id = Some(id.clone());
        }
        fixes.push(EloSentenceFix {
            id,
            char_start: start,
            char_end: end,
            original,
            corrected,
            alert_ids: members
                .iter()
                .map(|&index| alerts[index].id.clone())
                .collect(),
            segments,
        });
    }
    fixes
}

/// Clamps `value` to `[low, high]` without ever panicking.
///
/// Unlike `f64::clamp`, an inverted range (`low > high`) or a NaN bound is not a programming
/// error here: bounds come from live screen and field geometry. An inverted range resolves to
/// `high`; a NaN bound is ignored.
fn clamp_between(value: f64, low: f64, high: f64) -> f64 {
    value.max(low).min(high)
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

/// The text a suggestion puts in place of the flagged span.
fn suggestion_replacement(original: &str, suggestion: &Suggestion) -> String {
    match suggestion {
        Suggestion::ReplaceWith(r) => r.iter().collect::<String>(),
        Suggestion::InsertAfter(s) => format!("{original}{}", s.iter().collect::<String>()),
        Suggestion::Remove => String::new(),
    }
}

/// The name the frontend uses for a card command.
fn card_command_name(command: crate::config::CardCommand) -> &'static str {
    use crate::config::CardCommand;
    match command {
        CardCommand::Focus => "focus",
        CardCommand::NextSuggestion => "next",
        CardCommand::PreviousSuggestion => "prev",
        CardCommand::NextReplacement => "next_repl",
        CardCommand::PreviousReplacement => "prev_repl",
        CardCommand::Accept => "accept",
        CardCommand::Dismiss => "dismiss",
        CardCommand::Close => "close",
    }
}

/// Delivers a card command to the frontend at once.
///
/// The command rides on a single snapshot event carrying a fresh `card_command_seq`. The stored
/// snapshot never holds the command, so the frontend's poll of [`current_snapshot`] cannot replay
/// a keypress.
fn emit_card_command(app: &AppHandle, command: crate::config::CardCommand) {
    let snapshot = {
        let mut guard = lock_state();
        guard.snapshot.card_command_seq = guard.snapshot.card_command_seq.wrapping_add(1);
        let mut snapshot = guard.snapshot.clone();
        snapshot.card_command = Some(card_command_name(command).to_string());
        snapshot
    };
    let _ = app.emit("elo-overlay-snapshot", &snapshot);
}

/// An order-independent fingerprint of the user dictionary's words.
///
/// The cached linter is rebuilt when this changes, which catches a word being replaced as well
/// as one being added or removed (a word count alone misses the former).
fn dictionary_fingerprint(dictionary: &harper_core::spell::MutableDictionary) -> u64 {
    use harper_core::spell::Dictionary;
    use std::hash::{Hash, Hasher};

    let mut count = 0u64;
    let mut sum = 0u64;
    for word in dictionary.words_iter() {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        word.hash(&mut hasher);
        sum = sum.wrapping_add(hasher.finish());
        count += 1;
    }
    sum ^ count.rotate_left(32)
}

/// The broker's app policy: whether Elocutio may read the given app.
///
/// The config is often locked by an async task when the broker asks. Guessing "allowed" then
/// would read apps the user blocked, so a busy config falls back to the last decision made for
/// that app, and to "not allowed" for an app not seen before.
fn integration_policy(
    config: Arc<AsyncMutex<Config>>,
) -> impl FnMut(&str) -> bool + Send + Sync + 'static {
    let mut last_known: HashMap<String, bool> = HashMap::new();
    move |bundle_id| match config.try_lock() {
        Ok(mut cfg) => {
            let enabled = cfg.resolve_integration(bundle_id);
            last_known.insert(bundle_id.to_string(), enabled);
            enabled
        }
        Err(_) => last_known.get(bundle_id).copied().unwrap_or(false),
    }
}

/// Adds a dismissed lint to the ignore list and saves the config.
///
/// The config is used right away when it is free. When an async task holds it, the update is
/// queued on the async runtime instead of being dropped, and the overlay is refreshed once it
/// lands so the dismissed underline does not linger.
fn ignore_lint_in_config(config: &Arc<AsyncMutex<Config>>, lint: Lint, source_text: String) {
    fn ignore(cfg: &mut Config, lint: &Lint, source_text: &str) {
        let dict = Config::dictionary_from_user_dictionary(cfg.mutable_dictionary.clone());
        let doc = Document::new_markdown_default(source_text, &dict);
        cfg.ignored_lints.ignore_lint(lint, &doc);
    }

    let applied_now = match config.try_lock() {
        Ok(mut cfg) => {
            ignore(&mut cfg, &lint, &source_text);
            true
        }
        Err(_) => false,
    };
    let config = config.clone();
    tauri::async_runtime::spawn(async move {
        let mut cfg = config.lock().await;
        if !applied_now {
            ignore(&mut cfg, &lint, &source_text);
        }
        let _ = cfg.save_to_system().await;
        drop(cfg);
        if !applied_now {
            request_refresh();
        }
    });
}

/// Drops an alert from the stored snapshot once it has been acted on.
fn remove_alert_from_snapshot(alert_id: &str) {
    let mut guard = lock_state();
    guard.snapshot.alerts.retain(|a| a.id != alert_id);
    if guard.snapshot.alerts.is_empty() {
        guard.frontend_card_rect = None;
    }
}

type ActiveLints = Vec<(String, ActionableLint, Vec<Suggestion>)>;

/// Runs one command from the frontend. Returns whether the field should be re-read right away.
fn handle_command(
    command: OverlayCommand,
    broker: &mut crate::PlatformBroker,
    active_lints: &mut ActiveLints,
    config: &Arc<AsyncMutex<Config>>,
) -> bool {
    match command {
        OverlayCommand::ApplyAlert {
            alert_id,
            suggestion_index,
        } => {
            let Some(pos) = active_lints.iter().position(|(id, _, _)| *id == alert_id) else {
                return false;
            };
            let (_, mut actionable, suggestions) = active_lints.remove(pos);
            if let Some(suggestion) = suggestions
                .get(suggestion_index)
                .cloned()
                .or_else(|| suggestions.first().cloned())
            {
                // The macOS broker records the accepted edit itself, once, after the edit has
                // actually landed in the field.
                #[cfg(not(target_os = "macos"))]
                {
                    let chars: Vec<char> = actionable.source_text.chars().collect();
                    let original = actionable.lint.get_str(&chars);
                    let replacement = suggestion_replacement(&original, &suggestion);
                    let category = actionable.lint.lint_kind.to_string();
                    let _ = crate::voice::record_accepted(&original, &replacement, &category);
                }
                actionable.apply_suggestion(suggestion);
                let _ = crate::voice::refresh_feedback();
            }
            if let Ok(cfg) = config.try_lock() {
                let _ = crate::personalization_db::sync_and_status(&cfg);
            }
            remove_alert_from_snapshot(&alert_id);
            true
        }
        OverlayCommand::DismissAlert { alert_id } => dismiss_alert(&alert_id, active_lints, config),
        OverlayCommand::ReplaceFieldText { new_text } => {
            #[cfg(target_os = "macos")]
            {
                if let Err(error) = broker.replace_target_text(&new_text) {
                    eprintln!("Could not replace the field text: {error}");
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = &broker;
            }
            let mut guard = lock_state();
            guard.snapshot.source_text = new_text;
            guard.snapshot.alerts.clear();
            true
        }
        OverlayCommand::ApplySentenceFix { fix_id } => {
            let (fix, source_text) = {
                let guard = lock_state();
                (
                    guard
                        .snapshot
                        .sentence_fixes
                        .iter()
                        .find(|fix| fix.id == fix_id)
                        .cloned(),
                    guard.snapshot.source_text.clone(),
                )
            };
            let Some(fix) = fix else {
                return false;
            };
            #[cfg(target_os = "macos")]
            {
                let edit = crate::mac_broker::SpanEdit {
                    start: fix.char_start,
                    end: fix.char_end,
                    text: fix.corrected.clone(),
                    voice_replacement: Some(fix.corrected.clone()),
                    category: "SentenceCorrection".to_string(),
                    rule_name: "SentenceCorrection",
                };
                if let Err(error) = broker.replace_target_span(&source_text, edit) {
                    eprintln!("Could not apply the sentence correction: {error}");
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (&broker, &source_text);
            }
            active_lints.retain(|(id, _, _)| !fix.alert_ids.contains(id));
            let _ = crate::voice::refresh_feedback();
            let mut guard = lock_state();
            guard
                .snapshot
                .alerts
                .retain(|alert| !fix.alert_ids.contains(&alert.id));
            guard
                .snapshot
                .sentence_fixes
                .retain(|other| other.id != fix.id);
            if guard.snapshot.alerts.is_empty() {
                guard.frontend_card_rect = None;
            }
            true
        }
        OverlayCommand::DismissSentenceFix { fix_id } => {
            let alert_ids = {
                let mut guard = lock_state();
                let ids = guard
                    .snapshot
                    .sentence_fixes
                    .iter()
                    .find(|fix| fix.id == fix_id)
                    .map(|fix| fix.alert_ids.clone())
                    .unwrap_or_default();
                guard.snapshot.sentence_fixes.retain(|fix| fix.id != fix_id);
                ids
            };
            let mut changed = false;
            for alert_id in alert_ids {
                changed |= dismiss_alert(&alert_id, active_lints, config);
            }
            changed
        }
        OverlayCommand::ReplaceSelection {
            char_start,
            char_end,
            original,
            new_text,
        } => {
            let source_text = lock_state().snapshot.source_text.clone();
            let Some((start, end)) =
                locate_selection(&source_text, char_start, char_end, &original)
            else {
                eprintln!("The selection changed before the rewrite could be applied");
                return true;
            };
            #[cfg(target_os = "macos")]
            {
                let edit = crate::mac_broker::SpanEdit {
                    start,
                    end,
                    text: new_text,
                    // A generative rewrite is not a correction to learn word choices from.
                    voice_replacement: None,
                    category: "SelectionRewrite".to_string(),
                    rule_name: "SelectionRewrite",
                };
                if let Err(error) = broker.replace_target_span(&source_text, edit) {
                    eprintln!("Could not apply the selection rewrite: {error}");
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (&broker, start, end, new_text);
            }
            true
        }
        OverlayCommand::RefreshNow => true,
    }
}

/// Dismisses one alert: records the dismissal, ignores the lint from now on, and removes it from
/// the snapshot. Returns whether the alert was found.
fn dismiss_alert(
    alert_id: &str,
    active_lints: &mut ActiveLints,
    config: &Arc<AsyncMutex<Config>>,
) -> bool {
    let Some(pos) = active_lints.iter().position(|(id, _, _)| id == alert_id) else {
        return false;
    };
    let (_, actionable, suggestions) = active_lints.remove(pos);
    let chars: Vec<char> = actionable.source_text.chars().collect();
    let original = actionable.lint.get_str(&chars);
    let category = actionable.lint.lint_kind.to_string();
    let first_repl = suggestions
        .first()
        .map(|s| suggestion_replacement(&original, s))
        .unwrap_or_default();
    let _ = crate::voice::record_dismissed(&original, &first_repl, &category);
    let _ = crate::voice::refresh_feedback();
    crate::events::record_suggestion_event(
        crate::events::EventKind::Dismissed,
        &actionable.rule_name,
        actionable.lint.span.len(),
        0,
    );
    ignore_lint_in_config(config, actionable.lint, actionable.source_text);
    remove_alert_from_snapshot(alert_id);
    true
}

/// Where a selection the frontend saw now sits in `source_text`: at the same offsets when the
/// text there is unchanged, otherwise at the one place the selected text occurs.
fn locate_selection(
    source_text: &str,
    char_start: usize,
    char_end: usize,
    original: &str,
) -> Option<(usize, usize)> {
    let chars: Vec<char> = source_text.chars().collect();
    let wanted: Vec<char> = original.chars().collect();
    if wanted.is_empty() {
        return None;
    }
    if char_start <= char_end && chars.get(char_start..char_end) == Some(wanted.as_slice()) {
        return Some((char_start, char_end));
    }
    let mut matches = (0..=chars.len().saturating_sub(wanted.len()))
        .filter(|&position| chars[position..].starts_with(&wanted));
    let position = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some((position, position + wanted.len()))
}

/// The global cursor position in screen points.
fn global_cursor_position() -> Option<egui::Pos2> {
    #[cfg(target_os = "macos")]
    {
        use core_graphics::event::CGEvent;
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
        let location = CGEvent::new(source).ok()?.location();
        Some(egui::pos2(location.x as f32, location.y as f32))
    }
    #[cfg(target_os = "windows")]
    {
        crate::windows_broker::cursor_position_now()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Whether the cursor is over something the overlay must receive clicks for, plus the cursor in
/// overlay-local coordinates when it matters.
fn hit_test(state: &SharedOverlayState, cursor: Option<egui::Pos2>) -> (bool, Option<(f64, f64)>) {
    if !state.snapshot.service_enabled || !state.snapshot.field_active {
        return (false, None);
    }
    let logical = cursor.map(|cursor| {
        (
            f64::from(cursor.x) - state.monitor_origin.0,
            f64::from(cursor.y) - state.monitor_origin.1,
        )
    });
    if state.is_dragging {
        return (true, logical);
    }
    let Some((lx, ly)) = logical else {
        return (false, None);
    };

    let over_widget = state
        .frontend_widget_rect
        .or(state.snapshot.widget_rect)
        .is_some_and(|wr| wr.contains(lx, ly, 10.0));
    let over_card = state
        .frontend_card_rect
        .is_some_and(|cr| cr.contains(lx, ly, 14.0));
    let over_alert = state
        .snapshot
        .alerts
        .iter()
        .any(|a| a.rect.contains(lx, ly, 6.0));
    (over_widget || over_card || over_alert, logical)
}

/// Keeps the overlay click-through everywhere except over the `"elo"` widget, underlined words,
/// or the open suggestion card, and turns clicks there into frontend events.
///
/// This runs on its own thread, independent of the polling loop: a target app that stalls
/// Accessibility calls must never leave the overlay stuck clickable over that app.
fn run_hit_test_loop(app: &AppHandle) {
    let mut prev_mouse_down = false;
    let mut press_origin: Option<(f64, f64)> = None;

    loop {
        let idle = !OVERLAY_RUNNING.load(Ordering::Relaxed) || {
            let guard = lock_state();
            !guard.snapshot.field_active && !guard.is_dragging
        };
        thread::sleep(if idle {
            HIT_TEST_IDLE_INTERVAL
        } else {
            HIT_TEST_INTERVAL
        });

        let cursor_pos = global_cursor_position();
        let (cursor_over_interactive, logical_cursor) = hit_test(&lock_state(), cursor_pos);
        CURSOR_OVER_INTERACTIVE.store(cursor_over_interactive, Ordering::Relaxed);
        set_window_ignore_cursor(app, !cursor_over_interactive);

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
                let had_card = lock_state().frontend_card_rect.take().is_some();
                if OVERLAY_INPUT_FOCUSED.load(Ordering::Relaxed) {
                    set_overlay_input_focus(false);
                }
                if had_card {
                    let _ = app.emit("elo-overlay-outside-click", ());
                    request_refresh();
                }
            }
            if !mouse_down {
                press_origin = None;
            }
        }
        prev_mouse_down = mouse_down;
    }
}

/// The polling loop: reads the focused field over Accessibility, lints it, and publishes the
/// snapshot the `"elo-overlay"` frontend renders. Returns when a newer loop generation replaces
/// it or its command channel closes.
fn run_overlay_loop(
    app: &AppHandle,
    config: &Arc<AsyncMutex<Config>>,
    suggestion_count: &Arc<crate::config::SuggestionCount>,
    command_rx: &Receiver<OverlayCommand>,
    generation: u64,
) {
    let mut broker = crate::platform_broker(integration_policy(config.clone()));

    let mut active_lints: ActiveLints = Vec::new();
    let mut next_poll = Instant::now();
    let mut last_emitted_hash: u64 = 0;
    let mut cached_linter_state: Option<(
        harper_core::Dialect,
        u64,
        usize,
        harper_core::linting::LintGroup,
        Arc<harper_core::spell::MergedDictionary>,
    )> = None;
    let mut display_name_cache: HashMap<String, String> = HashMap::new();

    loop {
        if LOOP_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        heartbeat();

        // Sleep until the next poll is due, waking at once for a command.
        let mut force_refresh = false;
        match command_rx.recv_timeout(next_poll.saturating_duration_since(Instant::now())) {
            Ok(command) => {
                force_refresh |= handle_command(command, &mut broker, &mut active_lints, config);
                while let Ok(command) = command_rx.try_recv() {
                    force_refresh |=
                        handle_command(command, &mut broker, &mut active_lints, config);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        heartbeat();

        if !force_refresh && Instant::now() < next_poll {
            continue;
        }
        let idle = !OVERLAY_RUNNING.load(Ordering::Relaxed) || !lock_state().snapshot.field_active;
        next_poll = Instant::now()
            + if idle {
                IDLE_POLL_INTERVAL
            } else {
                ACTIVE_POLL_INTERVAL
            };

        // Card shortcuts are delivered before the freeze below, so they work while the card is
        // open, which is exactly when they are used.
        if let Some(command) = config
            .try_lock()
            .ok()
            .and_then(|mut cfg| cfg.card_command.take())
        {
            emit_card_command(app, command);
        }

        // While the user is dragging `"elo"`, hovering/clicking `"elo"`, or reviewing an open
        // suggestion/rewrite card (and hasn't focused the Studio window), keep the snapshot stable
        // so clicking on the overlay never loses the target text field or closes the card.
        if !force_refresh && !is_studio_window_focused() {
            let interacting = {
                let guard = lock_state();
                guard.snapshot.field_active
                    && (guard.is_dragging
                        || CURSOR_OVER_INTERACTIVE.load(Ordering::Relaxed)
                        || guard.frontend_card_rect.is_some()
                        || OVERLAY_INPUT_FOCUSED.load(Ordering::Relaxed))
            };
            if interacting {
                continue;
            }
        }

        let service_enabled = OVERLAY_RUNNING.load(Ordering::Relaxed);
        let ax_trusted =
            broker.accessibility_permission_status() == AccessibilityPermissionStatus::Granted;

        if !service_enabled || !ax_trusted {
            active_lints.clear();
            suggestion_count.set(0);
            {
                let mut guard = lock_state();
                guard.snapshot.service_enabled = service_enabled;
                guard.snapshot.ax_trusted = ax_trusted;
                guard.snapshot.field_active = false;
                guard.snapshot.alerts.clear();
                guard.snapshot.widget_rect = None;
                guard.snapshot.field_rect = None;
            }
            set_window_ignore_cursor(app, true);
            continue;
        }

        // Snapshot config for linting (reusing the compiled LintGroup unless dictionary/dialect changed)
        let ignored_lints = {
            let Ok(cfg) = config.try_lock() else {
                continue;
            };
            let dictionary_key = dictionary_fingerprint(&cfg.mutable_dictionary);
            let dialect = cfg.dialect;
            let rule_epoch = crate::commands::LINTER_RULE_EPOCH.load(Ordering::Relaxed);
            let needs_rebuild = cached_linter_state
                .as_ref()
                .is_none_or(|(d, key, ep, _, _)| {
                    *d != dialect || *key != dictionary_key || *ep != rule_epoch
                });
            if needs_rebuild {
                let dict = Config::dictionary_from_user_dictionary(cfg.mutable_dictionary.clone());
                let linter = cfg.create_linter();
                cached_linter_state = Some((dialect, dictionary_key, rule_epoch, linter, dict));
            }
            cfg.ignored_lints.clone()
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
        heartbeat();
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
            // On macOS the running process names itself; the broker's general lookup asks
            // Spotlight, which is far too slow for this thread.
            #[cfg(target_os = "macos")]
            let name = broker
                .focused_app_display_name()
                .unwrap_or_else(|| bundle_id.clone());
            #[cfg(not(target_os = "macos"))]
            let name = broker.integration_display_name(&bundle_id);
            display_name_cache.insert(bundle_id.clone(), name.clone());
            name
        };

        let cursor_pos = broker.cursor_position();
        let (mon_x, mon_y, mon_w, mon_h, main_h) =
            active_display_quartz_bounds(app, raw_field_rect, cursor_pos);
        let app_changed = lock_state().snapshot.app_bundle_id != bundle_id && has_active_field;
        sync_overlay_window_to_display(app, mon_x, mon_y, mon_w, mon_h, main_h, app_changed);

        let mut new_active_lints: ActiveLints = Vec::new();
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
                        Suggestion::InsertAfter(_) => {
                            let combined = suggestion_replacement(&original, s);
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
                    sentence_fix_id: None,
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

        let sentence_fixes = build_sentence_fixes(&source_text, &mut alerts);

        // The selection is only meaningful against the text it was read with.
        #[cfg(target_os = "macos")]
        let selection = broker
            .active_selection()
            .filter(|_| has_active_field)
            .map(|selection| EloSelection {
                char_start: selection.char_start,
                char_end: selection.char_end,
                text: selection.text,
                rect: EloRect::from_rect(selection.rect, mon_x, mon_y),
            });
        #[cfg(not(target_os = "macos"))]
        let selection: Option<EloSelection> = None;

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

        #[cfg(target_os = "macos")]
        let target_pid = broker.focused_pid().unwrap_or(0);
        #[cfg(not(target_os = "macos"))]
        let target_pid = 0;

        let snapshot = {
            let mut guard = lock_state();
            // The widget shows only while text is selected, just under the selection, and hides
            // when another window covers it.
            let widget_rect = selection
                .as_ref()
                .filter(|_| has_active_field)
                .map(|selection| compute_selection_pill_rect(selection.rect, mon_w, mon_h))
                .filter(|wr| {
                    !occluders.iter().any(|occ| {
                        wr.x < occ.x + occ.width
                            && wr.x + wr.width > occ.x
                            && wr.y < occ.y + occ.height
                            && wr.y + wr.height > occ.y
                    })
                });

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
                selection,
                sentence_fixes,
                card_command: None,
                card_command_seq: guard.snapshot.card_command_seq,
            };
            guard.snapshot = snap.clone();
            snap
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
            if let Some(selection) = &snapshot.selection {
                selection.char_start.hash(&mut hasher);
                selection.char_end.hash(&mut hasher);
            }
            for fix in &snapshot.sentence_fixes {
                fix.id.hash(&mut hasher);
                fix.corrected.hash(&mut hasher);
            }
            hasher.finish()
        };

        if state_hash != last_emitted_hash {
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

#[cfg(test)]
mod tests {
    use super::*;

    const MON_W: f64 = 1728.0;
    const MON_H: f64 = 1117.0;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> EloRect {
        EloRect {
            x,
            y,
            width,
            height,
        }
    }

    fn assert_on_screen(widget: EloRect) {
        assert!(
            (8.0..=MON_W - widget.width - 8.0).contains(&widget.x),
            "{widget:?}"
        );
        assert!(
            (32.0..=MON_H - widget.height - 8.0).contains(&widget.y),
            "{widget:?}"
        );
    }

    #[test]
    fn the_widget_sits_just_below_the_selection() {
        let widget = compute_selection_pill_rect(rect(200.0, 300.0, 240.0, 22.0), MON_W, MON_H);
        assert_eq!(widget.x, 200.0);
        assert_eq!(widget.y, 300.0 + 22.0 + PILL_GAP);
    }

    #[test]
    fn the_widget_flips_above_a_selection_at_the_bottom_of_the_screen() {
        let widget = compute_selection_pill_rect(rect(200.0, 1090.0, 240.0, 20.0), MON_W, MON_H);
        assert_eq!(widget.y, 1090.0 - PILL_HEIGHT - PILL_GAP);
        assert_on_screen(widget);
    }

    #[test]
    fn selections_at_the_screen_edges_do_not_panic() {
        for selection in [
            rect(100.0, -50.0, 400.0, 10.0),
            rect(100.0, 1200.0, 400.0, 20.0),
            rect(-500.0, 1100.0, 50.0, 17.0),
            rect(5000.0, 500.0, 50.0, 17.0),
            rect(f64::NAN, f64::NAN, 10.0, 10.0),
        ] {
            let widget = compute_selection_pill_rect(selection, MON_W, MON_H);
            assert!(widget.x.is_finite() && widget.y.is_finite(), "{widget:?}");
            if selection.x.is_finite() {
                assert_on_screen(widget);
            }
        }
    }

    fn sentences(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        sentence_ranges(&chars)
            .into_iter()
            .map(|(start, end)| chars[start..end].iter().collect())
            .collect()
    }

    #[test]
    fn text_splits_into_sentences() {
        assert_eq!(
            sentences("lets go. I want this! Done?  Yes\nNext line"),
            ["lets go.", "I want this!", "Done?", "Yes", "Next line"]
        );
        // A full stop inside a number or a file name does not end a sentence.
        assert_eq!(
            sentences("Pay 3.50 for v1.2 now."),
            ["Pay 3.50 for v1.2 now."]
        );
        assert!(sentences("   ").is_empty());
    }

    fn alert(
        id: &str,
        start: usize,
        end: usize,
        original: &str,
        kind: &str,
        repl: &str,
    ) -> EloAlert {
        EloAlert {
            id: id.to_string(),
            rule_name: "Test".to_string(),
            category: "correctness".to_string(),
            category_label: "Correctness".to_string(),
            subcategory: "Spelling".to_string(),
            color: "#E5484D".to_string(),
            original: original.to_string(),
            message: String::new(),
            context_before: String::new(),
            context_after: String::new(),
            suggestions: vec![EloSuggestion {
                kind: kind.to_string(),
                label: repl.to_string(),
                replacement: repl.to_string(),
            }],
            char_start: start,
            char_end: end,
            rect: EloRect::default(),
            sentence_fix_id: None,
        }
    }

    #[test]
    fn a_sentence_with_several_alerts_gets_one_correction() {
        let text = "lets worry about it later. It appears when someone select text.";
        let select_start = text.find("select").expect("the word is in the text");
        let text_start = text.find("text.").expect("the word is in the text");
        let mut alerts = vec![
            alert("a", 0, 4, "lets", "replace", "Let's"),
            alert(
                "b",
                select_start,
                select_start + 6,
                "select",
                "replace",
                "selects",
            ),
            alert(
                "c",
                text_start,
                text_start + 4,
                "text",
                "insert_after",
                "text,",
            ),
        ];
        let fixes = build_sentence_fixes(text, &mut alerts);

        // The first sentence has a single alert, so it keeps the word-level card.
        assert_eq!(fixes.len(), 1);
        let fix = &fixes[0];
        assert_eq!(fix.original, "It appears when someone select text.");
        assert_eq!(fix.corrected, "It appears when someone selects text,.");
        assert_eq!(fix.alert_ids, ["b", "c"]);
        assert_eq!(alerts[0].sentence_fix_id, None);
        assert_eq!(alerts[1].sentence_fix_id.as_deref(), Some(fix.id.as_str()));
        assert_eq!(alerts[2].sentence_fix_id.as_deref(), Some(fix.id.as_str()));

        let kinds: Vec<(&str, &str)> = fix
            .segments
            .iter()
            .map(|segment| (segment.kind.as_str(), segment.text.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("equal", "It appears when someone "),
                ("delete", "select"),
                ("insert", "selects"),
                ("equal", " text"),
                ("insert", ","),
                ("equal", "."),
            ]
        );
    }

    #[test]
    fn alerts_without_suggestions_do_not_make_a_sentence_correction() {
        let text = "teh cat sat on teh mat.";
        let mut alerts = vec![
            alert("a", 0, 3, "teh", "replace", "the"),
            alert("b", 15, 18, "teh", "replace", "the"),
        ];
        alerts[1].suggestions.clear();
        assert!(build_sentence_fixes(text, &mut alerts).is_empty());

        alerts[1] = alert("b", 15, 18, "teh", "replace", "the");
        let fixes = build_sentence_fixes(text, &mut alerts);
        assert_eq!(fixes[0].corrected, "the cat sat on the mat.");
    }

    #[test]
    fn a_selection_is_relocated_or_refused() {
        let text = "one two three two";
        // Unchanged offsets are trusted as they are.
        assert_eq!(locate_selection(text, 4, 7, "two"), Some((4, 7)));
        // Moved, and the text occurs twice: refused rather than guessed.
        assert_eq!(locate_selection(text, 0, 3, "two"), None);
        assert_eq!(locate_selection(text, 4, 13, "two three"), Some((4, 13)));
        // The text moved: found again at its one occurrence.
        assert_eq!(locate_selection(text, 0, 5, "three"), Some((8, 13)));
        assert_eq!(locate_selection(text, 0, 3, "four"), None);
        assert_eq!(locate_selection(text, 0, 0, ""), None);
    }

    #[test]
    fn clamp_between_tolerates_inverted_ranges() {
        assert_eq!(clamp_between(5.0, 0.0, 10.0), 5.0);
        assert_eq!(clamp_between(-5.0, 0.0, 10.0), 0.0);
        assert_eq!(clamp_between(50.0, 0.0, 10.0), 10.0);
        assert_eq!(clamp_between(5.0, 10.0, 0.0), 0.0);
    }

    fn active_state() -> SharedOverlayState {
        let mut state = SharedOverlayState::default();
        state.snapshot.field_active = true;
        state.snapshot.widget_rect = Some(rect(500.0, 500.0, 104.0, 38.0));
        state.monitor_origin = (100.0, 0.0);
        state
    }

    #[test]
    fn hit_testing_finds_the_widget_in_overlay_coordinates() {
        let state = active_state();
        let over_widget = egui::pos2(650.0, 520.0);
        assert_eq!(
            hit_test(&state, Some(over_widget)),
            (true, Some((550.0, 520.0)))
        );
        assert!(!hit_test(&state, Some(egui::pos2(10.0, 10.0))).0);
        assert!(!hit_test(&state, None).0);
    }

    #[test]
    fn hit_testing_is_click_through_when_there_is_nothing_to_click() {
        let mut state = active_state();
        state.snapshot.service_enabled = false;
        assert!(!hit_test(&state, Some(egui::pos2(650.0, 520.0))).0);

        let mut state = active_state();
        state.snapshot.field_active = false;
        assert!(!hit_test(&state, Some(egui::pos2(650.0, 520.0))).0);
    }

    #[test]
    fn dragging_keeps_the_overlay_clickable_everywhere() {
        let mut state = active_state();
        state.is_dragging = true;
        assert!(hit_test(&state, Some(egui::pos2(10.0, 10.0))).0);
    }

    #[test]
    fn a_heartbeat_goes_stale() {
        let stale_ms = u64::try_from(LOOP_STALE_AFTER.as_millis()).unwrap();
        assert!(!heartbeat_is_fresh(0, 10));
        assert!(heartbeat_is_fresh(1_000, 1_000));
        assert!(heartbeat_is_fresh(1_000, 1_000 + stale_ms - 1));
        assert!(!heartbeat_is_fresh(1_000, 1_000 + stale_ms));
    }

    #[test]
    fn restart_backoff_doubles_and_caps() {
        assert_eq!(restart_backoff(1), Duration::from_millis(250));
        assert_eq!(restart_backoff(2), Duration::from_millis(500));
        assert_eq!(restart_backoff(6), Duration::from_secs(8));
        assert_eq!(restart_backoff(60), Duration::from_secs(8));
    }

    #[test]
    fn a_supervised_thread_restarts_after_a_panic_then_gives_up() {
        use std::sync::atomic::AtomicU32;
        let runs = Arc::new(AtomicU32::new(0));
        let gave_up = Arc::new(AtomicBool::new(false));
        let body_runs = runs.clone();
        let flag = gave_up.clone();
        let handle = spawn_supervised(
            "supervised-test",
            move || {
                // Panic once, then finish normally.
                if body_runs.fetch_add(1, Ordering::SeqCst) == 0 {
                    panic!("first run fails");
                }
            },
            move || flag.store(true, Ordering::SeqCst),
        )
        .unwrap();
        handle.join().unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert!(!gave_up.load(Ordering::SeqCst));
    }

    #[test]
    fn the_dictionary_fingerprint_sees_replaced_words() {
        use harper_core::DictWordMetadata;
        use harper_core::spell::MutableDictionary;

        let dictionary = |words: &[&str]| {
            let mut dictionary = MutableDictionary::new();
            for word in words {
                dictionary.append_word_str(word, DictWordMetadata::default());
            }
            dictionary_fingerprint(&dictionary)
        };
        assert_eq!(
            dictionary(&["alpha", "beta"]),
            dictionary(&["beta", "alpha"])
        );
        // Same word count, different words: a count-based key missed this.
        assert_ne!(
            dictionary(&["alpha", "beta"]),
            dictionary(&["alpha", "gamma"])
        );
        assert_ne!(dictionary(&["alpha"]), dictionary(&["alpha", "beta"]));
    }

    #[test]
    fn a_busy_config_never_reads_an_unknown_or_blocked_app() {
        let config = Arc::new(AsyncMutex::new(Config::new()));
        config.try_lock().unwrap().block_app("com.example.Blocked");
        let mut policy = integration_policy(config.clone());
        assert!(policy("com.example.Editor"));
        assert!(!policy("com.example.Blocked"));

        let _busy = config.try_lock().unwrap();
        assert!(
            policy("com.example.Editor"),
            "falls back to the last decision"
        );
        assert!(!policy("com.example.Blocked"));
        assert!(!policy("com.example.Unseen"), "never guesses allowed");
    }

    #[test]
    fn card_commands_are_delivered_once_and_never_stored() {
        let mut state = SharedOverlayState::default();
        state.snapshot.card_command_seq = 7;
        let snapshot = serde_json::to_value(&state.snapshot).unwrap();
        assert_eq!(snapshot["card_command_seq"], 7);
        assert!(snapshot["card_command"].is_null());
        assert_eq!(
            card_command_name(crate::config::CardCommand::Accept),
            "accept"
        );
    }
}
