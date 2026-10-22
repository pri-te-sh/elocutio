use cached::cached;
mod accessibility_activation;
mod accessibility_text;
mod app_catalog;
mod app_icons;
mod core_foundation_utilities;
mod focused_target;
mod focused_window_pid;
mod window_stability;

use accessibility::TreeWalker;
use accessibility::attribute::AXUIElementAttributes;
use accessibility::ui_element::AXUIElement;
use accessibility_sys::{
    AXIsProcessTrusted, AXIsProcessTrustedWithOptions, kAXFocusedUIElementAttribute,
    kAXTrustedCheckOptionPrompt,
};
use accessibility_sys::{error_string, pid_t};
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use harper_core::linting::Lint;
use objc2_app_kit::NSRunningApplication;
use send_wrapper::SendWrapper;
use std::process::Command;
use std::time::Duration;
use std::{
    collections::{BTreeMap, HashMap},
    error::Error as StdError,
    sync::Mutex,
    time::Instant,
};

use crate::os_broker::{AccessibilityPermissionStatus, AppSearchResult, FocusedText, OsBroker};
use crate::rect::ActionableLint;

use self::accessibility_activation::{
    ACCESSIBILITY_ACTIVATION_RETRY_INTERVAL, AccessibilityActivationState,
    AccessibilityActivationStatus, AccessibilityActivationVerification,
    accessibility_activation_verification_retry_interval,
    is_unsupported_accessibility_activation_error, release_accessibility_activation,
    set_enhanced_user_interface_preserving_previous, verify_accessibility_activation,
};
use self::accessibility_text::{RectCollector, is_supported_text_element};
use self::core_foundation_utilities::{ax_element_attribute, ax_string_attribute};
use self::focused_target::FocusedTarget;
use self::window_stability::{
    WINDOW_MOVEMENT_SETTLE_DURATION, WindowMovementState, frontmost_window_frame_for_pid,
    settled_window_state, window_frame_changed,
};

/// Must match `identifier` in `tauri.conf.json`.
const BUNDLE_ID: &str = "app.elocutio.desktop";

/// macOS implementation of the OS data the highlighter needs.
///
/// `MacBroker` owns focus memory because clicking the overlay can make the highlighter process the
/// focused application. Remembering the last non-highlighter PID and exact UI element lets
/// accessibility reads continue targeting the field the user was reviewing.
pub struct MacBroker {
    /// Tauri requires a Send broker; retained AX handles must stay on their capturing thread.
    /// The wrapper enforces that restriction for access and drop. Only the highlighter populates it.
    last_focused: Option<SendWrapper<FocusedTarget>>,
    is_integration_enabled: Box<dyn FnMut(&str) -> bool + Send + Sync>,
    application_icon_cache: Mutex<HashMap<String, Vec<u8>>>,
    window_movement: Option<WindowMovementState>,
    accessibility_activation: Option<AccessibilityActivationState>,
    /// The tone detected for the focused field, for the widget's badge.
    detected_tone: Option<String>,
    /// Whether a supported editable text field was found on the last poll.
    has_active_field: bool,
    /// Screen-space bounding rectangle of the active text field (or its window).
    active_field_rect: Option<crate::rect::Rect>,
    /// Text of the active text field captured during the last `get_boxes` pass.
    active_field_text: Option<String>,
    /// Screen-space rectangles of any windows sitting in front of the target window.
    occluding_rects: Vec<crate::rect::Rect>,
}

impl MacBroker {
    /// Creates a broker with an app policy that may register newly encountered bundle IDs.
    /// The policy is called before reading the app's text and may change as settings are refreshed.
    pub fn new(is_integration_enabled: impl FnMut(&str) -> bool + Send + Sync + 'static) -> Self {
        Self {
            last_focused: None,
            is_integration_enabled: Box::new(is_integration_enabled),
            application_icon_cache: Mutex::new(HashMap::new()),
            window_movement: None,
            accessibility_activation: None,
            detected_tone: None,
            has_active_field: false,
            active_field_rect: None,
            active_field_text: None,
            occluding_rects: Vec::new(),
        }
    }

    /// Windows sitting in front of the active target window on screen.
    pub fn occluding_rects(&self) -> &[crate::rect::Rect] {
        &self.occluding_rects
    }

    /// Process ID of the currently remembered target application.
    pub fn focused_pid(&self) -> Option<i32> {
        self.last_focused.as_deref().map(|target| target.pid)
    }

    /// Text of the active text field from the most recent `get_boxes` pass.
    pub fn active_field_text(&self) -> Option<String> {
        self.active_field_text.clone()
    }

    /// Replaces the entire text of the currently remembered target text field (`last_focused` or
    /// `focused_text_element`) using `AXValue` write with a select-all + pasteboard fallback.
    pub fn replace_target_text(&mut self, new_text: &str) -> Result<bool, String> {
        let element = if let Some(target) = self.last_focused.as_deref()
            && let Some(ref el) = target.element
            && is_supported_text_element(el)
        {
            el.clone()
        } else {
            focused_text_element()?
        };

        let before = accessibility_text::read_element_text(&element).unwrap_or_default();
        let mut pid: pid_t = 0;
        let _ = unsafe {
            accessibility_sys::AXUIElementGetPid(element.as_concrete_TypeRef(), &mut pid)
        };

        let value = CFString::new(new_text);
        if element.set_value(value.as_CFType()).is_ok() {
            for attempt in 0..8 {
                if attempt > 0 {
                    std::thread::sleep(Duration::from_millis(22));
                }
                let after = accessibility_text::read_element_text(&element).unwrap_or_default();
                if after == new_text || (!before.is_empty() && after != before) {
                    if pid > 0 && !before.is_empty() {
                        let _ = crate::undo::record(pid, &before, new_text);
                    }
                    return Ok(true);
                }
            }
        }

        // Fallback: select all in the target app and paste via transient pasteboard
        if pid > 0
            && let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        {
            #[allow(deprecated)]
            let _ = app.activateWithOptions(
                objc2_app_kit::NSApplicationActivationOptions::ActivateIgnoringOtherApps,
            );
            std::thread::sleep(Duration::from_millis(35));
        }

        let snapshot = crate::pasteboard::write_transient(new_text)?;
        // Synthesize Cmd+A then Cmd+V
        if let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
            if let Ok(kd_a) = CGEvent::new_keyboard_event(source.clone(), 0, true) {
                kd_a.set_flags(CGEventFlags::CGEventFlagCommand);
                kd_a.post(CGEventTapLocation::HID);
            }
            if let Ok(ku_a) = CGEvent::new_keyboard_event(source, 0, false) {
                ku_a.set_flags(CGEventFlags::CGEventFlagCommand);
                ku_a.post(CGEventTapLocation::HID);
            }
        }
        std::thread::sleep(Duration::from_millis(40));
        let pasted = paste_command_v().is_ok();
        std::thread::sleep(Duration::from_millis(140));
        crate::pasteboard::restore(snapshot);

        if pasted && pid > 0 && !before.is_empty() {
            let _ = crate::undo::record(pid, &before, new_text);
        }
        Ok(pasted)
    }

    /// Refreshes the exact target during external focus and preserves it during overlay focus.
    ///
    /// The element lookup runs on every external-focus read, even within the same application.
    /// A failed lookup clears the remembered element; it must not silently retain a different field.
    fn resolve_target(
        &mut self,
        focused_pid: pid_t,
        read_element: impl FnOnce(pid_t) -> Option<AXUIElement>,
    ) -> Option<FocusedTarget> {
        // The check is by bundle identifier, not by pid. The overlay lives in
        // its own process, so the pid of the Elocutio application is not the
        // highlighter's pid, and a pid check would treat the overlay's own
        // focus as an external application.
        if !self.is_elocutio_process(focused_pid) {
            self.last_focused = Some(SendWrapper::new(FocusedTarget {
                pid: focused_pid,
                element: read_element(focused_pid),
            }));
        }

        self.last_focused.as_deref().cloned()
    }

    /// Whether a process is one of ours.
    ///
    /// The overlay runs in its own process but shares the application's bundle
    /// identifier, so the bundle is the reliable test for "this is us".
    fn is_elocutio_process(&self, pid: pid_t) -> bool {
        focused_window_pid::is_elocutio_pid(pid)
    }

    /// Check if the frontmost window for a given process is currently moving.
    fn window_is_moving(&mut self, pid: pid_t) -> bool {
        let Some(frame) = frontmost_window_frame_for_pid(pid) else {
            self.window_movement = None;
            return false;
        };

        let now = Instant::now();
        let Some(state) = &mut self.window_movement else {
            self.window_movement = Some(settled_window_state(pid, frame, now));
            return false;
        };

        if state.pid != pid {
            *state = settled_window_state(pid, frame, now);
            return false;
        }

        if window_frame_changed(state.frame, frame) {
            state.frame = frame;
            state.last_changed_at = now;
            return true;
        }

        now.duration_since(state.last_changed_at) < WINDOW_MOVEMENT_SETTLE_DURATION
    }

    /// Clears activation state and restores any saved AX attribute value.
    fn reset_accessibility_activation(&mut self) {
        if let Some(state) = self.accessibility_activation.take() {
            release_accessibility_activation(&state);
        }
    }

    /// Activates the focused app and waits until text range bounds are usable.
    fn ensure_accessibility_activation(
        &mut self,
        pid: pid_t,
        bundle_id: &str,
        app: &AXUIElement,
    ) -> bool {
        // Only Chromium/Electron apps need AXEnhancedUserInterface (or the
        // Electron-specific AXManualAccessibility) to expose geometry. Native
        // AppKit apps already expose text and can be read directly; walking
        // their full AX tree here can block the highlighter (Mail is the worst
        // offender) and starve every other app.
        if !needs_accessibility_activation(bundle_id) {
            if self
                .accessibility_activation
                .as_ref()
                .is_some_and(|state| state.pid == pid && state.bundle_id == bundle_id)
            {
                return true;
            }

            self.reset_accessibility_activation();
            self.accessibility_activation = Some(AccessibilityActivationState {
                pid,
                bundle_id: bundle_id.to_string(),
                status: AccessibilityActivationStatus::Ready,
                last_attempted_at: Instant::now(),
                enhanced_user_interface_restore_value: None,
            });
            return true;
        }

        let needs_new_activation = match &self.accessibility_activation {
            Some(state) => state.pid != pid || state.bundle_id != bundle_id,
            None => true,
        };

        if needs_new_activation {
            self.reset_accessibility_activation();
            return self.request_enhanced_user_interface(pid, bundle_id, app);
        }

        let Some(status) = self
            .accessibility_activation
            .as_ref()
            .map(|state| state.status)
        else {
            return self.request_enhanced_user_interface(pid, bundle_id, app);
        };

        let now = Instant::now();
        match status {
            AccessibilityActivationStatus::Ready => true,
            AccessibilityActivationStatus::Pending {
                ready_at,
                verification_attempts,
            } => {
                if now < ready_at {
                    return false;
                }

                let verification = verify_accessibility_activation(app);

                if matches!(
                    verification,
                    AccessibilityActivationVerification::FoundTextRangeBounds
                        | AccessibilityActivationVerification::FoundSupportedTextElement
                ) {
                    if let Some(state) = &mut self.accessibility_activation {
                        state.status = AccessibilityActivationStatus::Ready;
                    }
                    return true;
                }

                let next_verification_attempts = verification_attempts.saturating_add(1);

                if next_verification_attempts
                    >= accessibility_activation::ACCESSIBILITY_ACTIVATION_MAX_VERIFICATION_ATTEMPTS
                {
                    eprintln!(
                        "Accessibility activation for {bundle_id} pid {pid} never produced usable text bounds; pausing retries"
                    );
                    if let Some(state) = &mut self.accessibility_activation {
                        state.status = AccessibilityActivationStatus::RetryLater;
                        state.last_attempted_at = now;
                    }
                    return false;
                }

                let retry_interval = accessibility_activation_verification_retry_interval(
                    next_verification_attempts,
                );

                eprintln!(
                    "Accessibility activation for {bundle_id} pid {pid} is not ready for text metrics yet: {verification:?}; retrying verification in {} ms",
                    retry_interval.as_millis()
                );
                if let Some(state) = &mut self.accessibility_activation {
                    state.status = AccessibilityActivationStatus::Pending {
                        ready_at: instant_after(now, retry_interval),
                        verification_attempts: next_verification_attempts,
                    };
                }

                false
            }
            AccessibilityActivationStatus::RetryLater => {
                let Some(last_attempted_at) = self
                    .accessibility_activation
                    .as_ref()
                    .map(|state| state.last_attempted_at)
                else {
                    return self.request_enhanced_user_interface(pid, bundle_id, app);
                };

                if now.duration_since(last_attempted_at) < ACCESSIBILITY_ACTIVATION_RETRY_INTERVAL {
                    return false;
                }

                self.reset_accessibility_activation();
                self.request_enhanced_user_interface(pid, bundle_id, app)
            }
        }
    }

    /// Requests `AXEnhancedUserInterface`, preserving the previous value if readable.
    fn request_enhanced_user_interface(
        &mut self,
        pid: pid_t,
        bundle_id: &str,
        app: &AXUIElement,
    ) -> bool {
        let now = Instant::now();
        let _settle_duration = accessibility_activation::CHROMIUM_ACCESSIBILITY_SETTLE_DURATION;

        match set_enhanced_user_interface_preserving_previous(app, true) {
            Ok(enhanced_user_interface_restore_value) => {
                self.accessibility_activation = Some(AccessibilityActivationState {
                    pid,
                    bundle_id: bundle_id.to_string(),
                    status: AccessibilityActivationStatus::Ready,
                    last_attempted_at: now,
                    enhanced_user_interface_restore_value,
                });
                true
            }
            Err(error) if is_unsupported_accessibility_activation_error(error) => {
                self.accessibility_activation = Some(AccessibilityActivationState {
                    pid,
                    bundle_id: bundle_id.to_string(),
                    status: AccessibilityActivationStatus::Ready,
                    last_attempted_at: now,
                    enhanced_user_interface_restore_value: None,
                });
                true
            }
            Err(error) => {
                eprintln!(
                    "Unable to request AXEnhancedUserInterface for {bundle_id} pid {pid}: {}",
                    error_string(error)
                );
                self.accessibility_activation = Some(AccessibilityActivationState {
                    pid,
                    bundle_id: bundle_id.to_string(),
                    status: AccessibilityActivationStatus::Ready,
                    last_attempted_at: now,
                    enhanced_user_interface_restore_value: None,
                });
                true
            }
        }
    }
}

/// Whether an app needs an accessibility activation nudge to expose text geometry.
fn needs_accessibility_activation(bundle_id: &str) -> bool {
    !bundle_id.starts_with("com.apple.")
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> bool;
}

/// Returns the currently focused editable text element, refusing secure input.
fn focused_text_element() -> Result<AXUIElement, String> {
    if unsafe { IsSecureEventInputEnabled() } {
        return Err("Secure input is active; refusing to read the focused field.".into());
    }

    let pid = focused_window_pid::focused_text_pid()
        .or_else(|_| focused_window_pid::focused_window_pid())
        .map_err(|error| error.to_string())?;
    let app = AXUIElement::application(pid);
    let element = ax_element_attribute(&app, kAXFocusedUIElementAttribute)
        .map_err(|error| error.to_string())?;

    if !is_supported_text_element(&element) {
        return Err("The focused element is not an editable text field.".into());
    }

    if ax_string_attribute(&element, "AXSubrole")
        .map(|subrole| subrole == "AXSecureTextField")
        .unwrap_or(false)
    {
        return Err("Refusing to read a secure text field.".into());
    }

    Ok(element)
}

/// Synthesizes ⌘V into the frontmost app.
fn paste_command_v() -> Result<(), String> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "Could not create a keyboard event source.".to_string())?;
    let key_down = CGEvent::new_keyboard_event(source, 9, true)
        .map_err(|_| "Could not create the paste key-down event.".to_string())?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);
    key_down.post(CGEventTapLocation::HID);

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "Could not create a keyboard event source.".to_string())?;
    let key_up = CGEvent::new_keyboard_event(source, 9, false)
        .map_err(|_| "Could not create the paste key-up event.".to_string())?;
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);
    key_up.post(CGEventTapLocation::HID);

    Ok(())
}

impl Drop for MacBroker {
    fn drop(&mut self) {
        self.reset_accessibility_activation();
    }
}

pub(super) type LintCallback<'a> = dyn FnMut(&str) -> BTreeMap<String, Vec<Lint>> + 'a;

impl OsBroker for MacBroker {
    fn is_elocutio(app_id: &str) -> bool {
        app_id == BUNDLE_ID
    }

    fn get_boxes(&mut self, lint_text: &mut LintCallback) -> Option<Vec<ActionableLint>> {
        let focused_pid = match focused_window_pid::focused_text_pid() {
            Ok(pid) => pid,
            Err(_) => {
                self.window_movement = None;
                self.has_active_field = false;
                self.active_field_rect = None;
                self.detected_tone = None;
                self.reset_accessibility_activation();
                return None;
            }
        };
        let Some(target) = self.resolve_target(focused_pid, |pid| {
            let app = AXUIElement::application(pid);
            let raw_focused = ax_element_attribute(&app, kAXFocusedUIElementAttribute)
                .ok()
                .or_else(|| {
                    let sys = AXUIElement::system_wide();
                    ax_element_attribute(&sys, kAXFocusedUIElementAttribute)
                        .ok()
                        .filter(|el| {
                            let mut el_pid: pid_t = 0;
                            unsafe {
                                accessibility_sys::AXUIElementGetPid(
                                    el.as_concrete_TypeRef(),
                                    &mut el_pid,
                                ) == accessibility_sys::kAXErrorSuccess
                                    && el_pid == pid
                            }
                        })
                });
            if raw_focused.as_ref().is_some_and(|el| {
                ax_string_attribute(el, "AXSubrole")
                    .is_ok_and(|subrole| subrole == "AXSecureTextField")
            }) {
                return None;
            }
            raw_focused
                .as_ref()
                .and_then(accessibility_text::enclosing_supported_text_element)
        }) else {
            self.window_movement = None;
            self.has_active_field = false;
            self.active_field_rect = None;
            self.detected_tone = None;
            self.reset_accessibility_activation();
            return Some(Vec::new());
        };
        let pid = target.pid;

        let bundle_identifier = match bundle_identifier_for_pid(pid) {
            Ok(Some(bundle_identifier)) => bundle_identifier,
            Ok(None) => {
                self.window_movement = None;
                self.has_active_field = false;
                self.active_field_rect = None;
                self.detected_tone = None;
                self.reset_accessibility_activation();
                return None;
            }
            Err(error) => {
                self.window_movement = None;
                self.has_active_field = false;
                self.active_field_rect = None;
                self.detected_tone = None;
                self.reset_accessibility_activation();
                eprintln!("Unable to identify focused app bundle: {error}");
                return None;
            }
        };

        if !(self.is_integration_enabled)(&bundle_identifier) {
            self.window_movement = None;
            self.has_active_field = false;
            self.active_field_rect = None;
            self.detected_tone = None;
            self.reset_accessibility_activation();
            return Some(Vec::new());
        }

        // Hide highlights while window is moving to avoid "sliding" behavior.
        if self.window_is_moving(pid) {
            self.has_active_field = false;
            return Some(Vec::new());
        }

        let el = AXUIElement::application(pid);
        if !self.ensure_accessibility_activation(pid, &bundle_identifier, &el) {
            self.has_active_field = false;
            return None;
        }

        let Some(focused) = target.traversal_root() else {
            self.has_active_field = false;
            self.active_field_rect = None;
            self.detected_tone = None;
            return Some(Vec::new());
        };

        let walker = TreeWalker::new();
        let cached = match crate::llm_cache::read() {
            Ok(cached) => cached,
            Err(error) => {
                eprintln!("LLM cache read failed: {error}");
                None
            }
        };
        let collector = RectCollector::new(lint_text, cached, pid);

        walker.walk(&focused, &collector);

        self.has_active_field = collector.found_text_element();
        self.active_field_text = collector.source_text();
        let field_candidate = collector.field_rect();
        let win_frame = window_stability::window_frame_for_pid(pid, field_candidate);
        let raw_field = match (field_candidate, win_frame) {
            (Some(fr), Some(wf)) => fr.intersection(wf).or(Some(fr)),
            (Some(fr), None) => Some(fr),
            (None, Some(wf)) => Some(wf),
            (None, None) => None,
        };
        let occluders = window_stability::occluding_window_rects_above_pid(pid);
        self.occluding_rects = occluders.clone();
        self.active_field_rect =
            raw_field.and_then(|fr| clip_field_against_occluders(fr, &occluders));

        // If the active field is mostly or completely covered by an occluding window in front of it,
        // or has no visible area left, hide the overlay so it never bleeds over frontmost windows.
        if self.active_field_rect.is_none() {
            self.has_active_field = false;
            self.detected_tone = None;
            return Some(Vec::new());
        }

        self.detected_tone = collector.detected_tone();
        let mut result = collector.unwrap_rects();
        if let Some(ref mut lints) = result {
            let field_bounds = self.active_field_rect;
            lints.retain_mut(|item| {
                if let Some(fb) = field_bounds {
                    let Some(clipped) = item.rect.intersection(fb) else {
                        return false;
                    };
                    if clipped.width < 6.0 || clipped.height < 8.0 {
                        return false;
                    }
                    item.rect = clipped;
                }
                !occluders.iter().any(|occ| item.rect.intersects(*occ))
            });
        }

        thread_local! {
            static LAST_DIAG: std::cell::RefCell<(pid_t, bool, bool, usize)> = const {
                std::cell::RefCell::new((0, false, false, usize::MAX))
            };
        }
        let trusted = unsafe { AXIsProcessTrusted() };
        let rect_count = result.as_ref().map_or(0, Vec::len);
        LAST_DIAG.with(|diag| {
            let state = (pid, trusted, self.has_active_field, rect_count);
            if *diag.borrow() != state {
                *diag.borrow_mut() = state;
                eprintln!(
                    "Highlighter target: bundle={bundle_identifier} pid={pid} ax_trusted={trusted} active_field={} lints={rect_count}",
                    self.has_active_field
                );
            }
        });

        result
    }

    fn selected_text(&mut self) -> Result<Option<String>, String> {
        let focused = focused_text_element()?;
        let selected =
            ax_string_attribute(&focused, "AXSelectedText").map_err(|error| error.to_string())?;

        Ok((!selected.is_empty()).then_some(selected))
    }

    fn replace_focused_text(&mut self, text: &str) -> Result<bool, String> {
        let element = focused_text_element()?;
        let value = CFString::new(text);
        element
            .set_value(value.as_CFType())
            .map_err(|error| format!("Could not restore the field: {error}"))?;
        let actual = ax_string_attribute(&element, "AXValue")
            .map_err(|error| format!("Could not verify the restored field: {error}"))?;
        Ok(actual == text)
    }

    fn detected_tone(&self) -> Option<String> {
        self.detected_tone.clone()
    }

    fn has_active_field(&self) -> bool {
        self.has_active_field
    }

    fn active_field_rect(&self) -> Option<crate::rect::Rect> {
        self.active_field_rect
    }

    fn focused_bundle_id(&mut self) -> Result<Option<String>, String> {
        let pid = self
            .last_focused
            .as_deref()
            .map(|target| target.pid)
            .or_else(|| focused_window_pid::focused_text_pid().ok())
            .or_else(|| focused_window_pid::focused_window_pid().ok())
            .ok_or_else(|| "Unable to identify focused window.".to_string())?;
        bundle_identifier_for_pid(pid).map_err(|error| error.to_string())
    }

    fn focused_text(&mut self) -> Result<Option<FocusedText>, String> {
        if unsafe { IsSecureEventInputEnabled() } {
            return Err("Secure input is active; refusing to read the focused field.".into());
        }

        let pid = focused_window_pid::focused_text_pid()
            .or_else(|_| focused_window_pid::focused_window_pid())
            .map_err(|error| error.to_string())?;
        let bundle_id = bundle_identifier_for_pid(pid)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Could not identify the focused app bundle.".to_string())?;
        if Self::is_elocutio(&bundle_id) {
            return Ok(None);
        }
        let app = AXUIElement::application(pid);
        let element = ax_element_attribute(&app, kAXFocusedUIElementAttribute)
            .map_err(|error| error.to_string())?;

        if !is_supported_text_element(&element) {
            return Ok(None);
        }
        if ax_string_attribute(&element, "AXSubrole")
            .map(|subrole| subrole == "AXSecureTextField")
            .unwrap_or(false)
        {
            return Err("Refusing to read a secure text field.".into());
        }

        let Some(text) = accessibility_text::read_element_text(&element) else {
            return Ok(None);
        };
        if text.is_empty() {
            return Ok(None);
        }

        Ok(Some(FocusedText {
            pid,
            bundle_id,
            text,
        }))
    }

    fn replace_selection_via_paste(&mut self, replacement: &str) -> Result<bool, String> {
        let focused = focused_text_element()?;
        let selected =
            ax_string_attribute(&focused, "AXSelectedText").map_err(|error| error.to_string())?;

        if selected.is_empty() {
            return Err("Select some text in the target app first.".into());
        }

        // Remember the field value before the paste so the read-back can prove
        // that the replacement landed in the selected span, and nowhere else.
        let before = ax_string_attribute(&focused, "AXValue").ok();

        let clipboard_snapshot = crate::pasteboard::write_transient(replacement)?;

        paste_command_v()?;

        // Give the target app a moment to process the synthetic paste before
        // reading the field back and restoring the clipboard.
        std::thread::sleep(Duration::from_millis(180));
        let after = ax_string_attribute(&focused, "AXValue").ok();
        let verified = match (before, after) {
            (Some(before), Some(after)) => {
                verify_paste_result(&before, &selected, replacement, &after)
            }
            // Without a pre-paste value we cannot prove the edit, so treat the
            // paste as unverified rather than guessing.
            _ => false,
        };

        // A lossy restore means some pasteboard types could not be captured.
        // Say so rather than silently losing the user's clipboard content.
        if !clipboard_snapshot.is_lossless() {
            eprintln!(
                "Clipboard restore was incomplete; these types could not be preserved: {}",
                clipboard_snapshot.unsupported_types().join(", ")
            );
        }
        crate::pasteboard::restore(clipboard_snapshot);

        Ok(verified)
    }

    fn cursor_position(&self) -> Option<egui::Pos2> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
        let event = CGEvent::new(source).ok()?;
        let location = event.location();

        Some(egui::pos2(location.x as f32, location.y as f32))
    }

    fn accessibility_permission_status(&self) -> AccessibilityPermissionStatus {
        if unsafe { AXIsProcessTrusted() } {
            AccessibilityPermissionStatus::Granted
        } else {
            AccessibilityPermissionStatus::NotGranted
        }
    }

    fn request_accessibility_permission(&self) -> AccessibilityPermissionStatus {
        let prompt_key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
        let prompt_value = CFBoolean::true_value();
        let options: CFDictionary<CFString, CFBoolean> =
            CFDictionary::from_CFType_pairs(&[(prompt_key, prompt_value)]);

        if unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) } {
            AccessibilityPermissionStatus::Granted
        } else {
            AccessibilityPermissionStatus::NotGranted
        }
    }

    fn integration_display_name(&self, bundle_id: &str) -> String {
        app_catalog::integration_display_name(bundle_id)
    }

    fn installed_application_bundle_ids(&self) -> Result<Vec<String>, String> {
        let ids = app_catalog::installed_application_bundle_ids()?;
        Ok(ids.iter().cloned().collect())
    }

    fn application_icon_png(&self, bundle_id: &str) -> Result<Vec<u8>, String> {
        let bundle_id = bundle_id.trim();

        if bundle_id.is_empty() {
            return Err("Bundle ID cannot be empty.".to_string());
        }

        if let Some(icon_png) = self
            .application_icon_cache
            .lock()
            .map_err(|error| format!("Failed to read application icon cache: {error}"))?
            .get(bundle_id)
            .cloned()
        {
            return Ok(icon_png);
        }

        let icon_png = app_icons::application_icon_png(bundle_id)?;
        self.application_icon_cache
            .lock()
            .map_err(|error| format!("Failed to update application icon cache: {error}"))?
            .insert(bundle_id.to_string(), icon_png.clone());

        Ok(icon_png)
    }

    fn launch_app_bundle(&self, bundle_id: &str) -> Result<(), String> {
        let bundle_id: &str = bundle_id;
        let bundle_id = bundle_id.trim();
        if bundle_id.is_empty() {
            return Err("Bundle ID cannot be empty.".to_string());
        }
        Command::new("open")
            .arg("-b")
            .arg(bundle_id)
            .spawn()
            .map_err(|error| format!("Failed to launch {bundle_id}: {error}"))?;
        Ok(())
    }

    fn search_apps(&self, query: &str) -> Result<Vec<AppSearchResult>, String> {
        let list = app_catalog::installed_application_search_results()?;

        let query = query.trim();

        if query.is_empty() {
            return Ok(list.to_vec());
        }

        if let Some(result) = list
            .iter()
            .find(|result| result.bundle_id == query)
            .cloned()
        {
            return Ok(vec![result]);
        }

        let lower_query = query.to_lowercase();
        Ok(list
            .iter()
            .filter(|result| {
                result.name.to_lowercase().contains(&lower_query)
                    || result.bundle_id.to_lowercase().contains(&lower_query)
            })
            .cloned()
            .collect())
    }
}

#[cached(max_size = 1_000)]
fn bundle_identifier_for_pid(pid: pid_t) -> Result<Option<String>, Box<dyn StdError>> {
    let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
        return Ok(None);
    };
    let Some(bundle_identifier) = app.bundleIdentifier() else {
        return Ok(None);
    };

    Ok(Some(bundle_identifier.to_string()))
}

/// Adds a duration to an instant without panicking on overflow.
fn instant_after(now: Instant, duration: Duration) -> Instant {
    now.checked_add(duration).unwrap_or(now)
}

/// Shrinks `field` against any occluding windows in front of it so that neither underlines
/// nor the `"elo"` widget bleed onto overlapping windows.
fn clip_field_against_occluders(
    mut field: crate::rect::Rect,
    occluders: &[crate::rect::Rect],
) -> Option<crate::rect::Rect> {
    for occ in occluders {
        let Some(inter) = field.intersection(*occ) else {
            continue;
        };
        let field_area = (field.width * field.height).max(1.0);
        let inter_area = inter.width * inter.height;
        if inter_area / field_area > 0.8 {
            return None;
        }

        // If the occluder spans most of the vertical height of the field, trim horizontally
        if inter.height >= field.height * 0.65 {
            let left_rem = (inter.x - field.x).max(0.0);
            let right_rem = ((field.x + field.width) - (inter.x + inter.width)).max(0.0);
            if left_rem >= right_rem && left_rem >= 64.0 {
                field.width = left_rem;
            } else if right_rem > left_rem && right_rem >= 64.0 {
                field.x = inter.x + inter.width;
                field.width = right_rem;
            } else {
                return None;
            }
        } else if inter.width >= field.width * 0.65 {
            // If the occluder spans most of the horizontal width of the field, trim vertically
            let top_rem = (inter.y - field.y).max(0.0);
            let bottom_rem = ((field.y + field.height) - (inter.y + inter.height)).max(0.0);
            if top_rem >= bottom_rem && top_rem >= 24.0 {
                field.height = top_rem;
            } else if bottom_rem > top_rem && bottom_rem >= 24.0 {
                field.y = inter.y + inter.height;
                field.height = bottom_rem;
            } else {
                return None;
            }
        }
    }

    (field.width >= 64.0 && field.height >= 18.0).then_some(field)
}

/// Proves that a synthetic paste replaced exactly the selected span.
///
/// The check is structural, not a substring test: the field value before the
/// paste must contain the selected text, and the value after the paste must
/// equal the value before with that one occurrence of the selection replaced.
/// A paste that duplicated text, landed in the wrong place, or left residue
/// behind fails this check, which is what a read-back verification should do.
///
/// The selection may appear more than once in the field. Every occurrence is
/// replaced in the expectation, because a host app that keeps the selection
/// range may paste over all of them. The expectation is then compared to the
/// observed value.
fn verify_paste_result(before: &str, selected: &str, replacement: &str, after: &str) -> bool {
    if selected.is_empty() {
        return false;
    }

    let occurrences = before.matches(selected).count();
    if occurrences == 0 {
        return false;
    }

    let expected = before.replace(selected, replacement);
    expected == after
}

#[cfg(test)]
mod paste_verification_tests {
    use super::verify_paste_result;

    #[test]
    fn accepts_a_clean_replacement() {
        assert!(verify_paste_result(
            "hello tehre world",
            "tehre",
            "there",
            "hello there world"
        ));
    }

    #[test]
    fn rejects_leftover_text_after_the_replacement() {
        // The old check used `contains`, so this passed even though the paste
        // left the original text behind.
        assert!(!verify_paste_result(
            "hello tehre world",
            "tehre",
            "there",
            "hello there tehre world"
        ));
    }

    #[test]
    fn rejects_a_paste_into_the_wrong_place() {
        assert!(!verify_paste_result(
            "hello tehre world",
            "tehre",
            "there",
            "there hello tehre world"
        ));
    }

    #[test]
    fn rejects_an_unchanged_field() {
        assert!(!verify_paste_result(
            "hello tehre world",
            "tehre",
            "there",
            "hello tehre world"
        ));
    }

    #[test]
    fn rejects_a_selection_that_was_not_in_the_field() {
        assert!(!verify_paste_result(
            "hello world",
            "tehre",
            "there",
            "hello there world"
        ));
    }

    #[test]
    fn rejects_an_empty_selection() {
        assert!(!verify_paste_result(
            "hello world",
            "",
            "there",
            "hello world"
        ));
    }

    #[test]
    fn handles_every_occurrence_of_the_selection() {
        // Some hosts keep a range that matches more than one place; the paste
        // then replaces all of them.
        assert!(verify_paste_result(
            "tehre and tehre",
            "tehre",
            "there",
            "there and there"
        ));
    }

    #[test]
    fn handles_an_empty_replacement() {
        assert!(verify_paste_result(
            "hello tehre world",
            "tehre",
            "",
            "hello  world"
        ));
    }
}
