use accessibility::attribute::AXUIElementAttributes;
use accessibility::ui_element::AXUIElement;
use accessibility::{TreeVisitor, TreeWalker, TreeWalkerFlow};
use accessibility_sys::{
    AXUIElementCopyAttributeValue, AXUIElementSetAttributeValue, error_string,
    kAXErrorAttributeUnsupported, kAXErrorNoValue, kAXErrorNotImplemented, kAXErrorSuccess, pid_t,
};
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::CFString;
use std::{
    cell::Cell,
    ptr,
    sync::Mutex,
    time::{Duration, Instant},
};

use super::accessibility_text::{
    cf_type_to_string, element_rect_for_text_range_with_fallback, is_supported_text_element,
    probe_element_rect_for_text_range,
};

pub const ACCESSIBILITY_ACTIVATION_RETRY_INTERVAL: Duration = Duration::from_secs(10);
pub const ACCESSIBILITY_ACTIVATION_VERIFICATION_RETRY_INTERVAL: Duration =
    Duration::from_millis(250);
pub const ACCESSIBILITY_ACTIVATION_SLOW_VERIFICATION_RETRY_INTERVAL: Duration =
    Duration::from_secs(1);
/// Stop burning AX calls on an app that exposes text but never usable bounds.
pub const ACCESSIBILITY_ACTIVATION_MAX_VERIFICATION_ATTEMPTS: u8 = 40;
pub const ACCESSIBILITY_ACTIVATION_FAST_VERIFICATION_ATTEMPTS: u8 = 20;

/// Tracks an app accessibility activation attempt and any AX value Elocutio should restore later.
#[derive(Debug, Clone)]
pub struct AccessibilityActivationState {
    pub pid: pid_t,
    pub bundle_id: String,
    pub status: AccessibilityActivationStatus,
    pub last_attempted_at: Instant,
    /// What Elocutio changed on the app, so it can be put back when the user leaves it.
    pub changes: ActivationChanges,
}

/// The accessibility attributes Elocutio switched on for an app, with the values to restore.
///
/// `None` means the attribute was not touched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActivationChanges {
    pub manual_accessibility_restore_value: Option<bool>,
    pub enhanced_user_interface_restore_value: Option<bool>,
}

impl ActivationChanges {
    fn is_empty(&self) -> bool {
        self.manual_accessibility_restore_value.is_none()
            && self.enhanced_user_interface_restore_value.is_none()
    }
}

/// The activation currently applied to some app, kept process-wide so it can be undone at quit
/// even though the broker that applied it lives on the overlay thread.
static APPLIED_ACTIVATION: Mutex<Option<(pid_t, ActivationChanges)>> = Mutex::new(None);

fn applied_activation() -> std::sync::MutexGuard<'static, Option<(pid_t, ActivationChanges)>> {
    APPLIED_ACTIVATION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// State of the focused app's accessibility activation attempt.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum AccessibilityActivationStatus {
    Pending {
        ready_at: Instant,
        verification_attempts: u8,
    },
    Ready,
    RetryLater,
}

/// Result of checking whether an activated app exposes usable text geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibilityActivationVerification {
    FoundTextRangeBounds,
    FoundSupportedTextElement,
    NoSupportedTextElement,
}

pub fn accessibility_activation_verification_retry_interval(verification_attempts: u8) -> Duration {
    if verification_attempts <= ACCESSIBILITY_ACTIVATION_FAST_VERIFICATION_ATTEMPTS {
        ACCESSIBILITY_ACTIVATION_VERIFICATION_RETRY_INTERVAL
    } else {
        ACCESSIBILITY_ACTIVATION_SLOW_VERIFICATION_RETRY_INTERVAL
    }
}

/// Puts back whatever accessibility attributes Elocutio switched on for the state's app.
pub fn release_accessibility_activation(state: &AccessibilityActivationState) {
    {
        let mut applied = applied_activation();
        if applied.is_some_and(|(pid, _)| pid == state.pid) {
            *applied = None;
        }
    }
    restore_changes(state.pid, state.changes);
}

/// Restores the activation applied to whichever app was last targeted. Called at quit, when the
/// overlay thread's broker will not get the chance to clean up after itself.
pub fn release_applied_activation() {
    let applied = applied_activation().take();
    if let Some((pid, changes)) = applied {
        restore_changes(pid, changes);
    }
}

fn restore_changes(pid: pid_t, changes: ActivationChanges) {
    if changes.is_empty() {
        return;
    }

    let app = super::core_foundation_utilities::application_element(pid);
    restore_boolean_attribute(
        &app,
        "AXEnhancedUserInterface",
        changes.enhanced_user_interface_restore_value,
    );
    restore_boolean_attribute(
        &app,
        "AXManualAccessibility",
        changes.manual_accessibility_restore_value,
    );
}

/// Chromium browsers that only build their accessibility tree for `AXEnhancedUserInterface`.
///
/// Electron apps (and current Chromium) honour `AXManualAccessibility`, which switches on the
/// accessibility tree and nothing else. `AXEnhancedUserInterface` also makes AppKit drop window
/// animations and confuses window managers, so it is reserved for the browsers that need it.
const ENHANCED_USER_INTERFACE_BUNDLE_PREFIXES: &[&str] = &[
    "com.google.Chrome",
    "org.chromium.Chromium",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    "com.vivaldi.Vivaldi",
    "com.operasoftware.Opera",
    "company.thebrowser.",
];

/// Whether the app needs `AXEnhancedUserInterface` on top of `AXManualAccessibility`.
pub fn needs_enhanced_user_interface(bundle_id: &str) -> bool {
    ENHANCED_USER_INTERFACE_BUNDLE_PREFIXES
        .iter()
        .any(|prefix| bundle_id.starts_with(prefix))
}

/// Returns whether an AX error means the target does not support this attribute.
pub fn is_unsupported_accessibility_activation_error(error: i32) -> bool {
    error == kAXErrorAttributeUnsupported
        || error == kAXErrorNoValue
        || error == kAXErrorNotImplemented
}

/// Switches on the app's accessibility tree and returns what was changed, for restoring later.
///
/// `AXManualAccessibility` is always tried first. `AXEnhancedUserInterface` is only set for the
/// Chromium browsers that need it (see [`needs_enhanced_user_interface`]). An unknown previous
/// value is restored as `false`, the default for both attributes, since Elocutio set it to `true`.
pub fn activate_accessibility(
    app: &AXUIElement,
    bundle_id: &str,
) -> Result<ActivationChanges, i32> {
    let mut changes = ActivationChanges::default();

    let manual = set_boolean_attribute_preserving_previous(app, "AXManualAccessibility", true);
    if let Ok(previous) = manual {
        changes.manual_accessibility_restore_value = Some(previous.unwrap_or(false));
    }

    if needs_enhanced_user_interface(bundle_id) {
        match set_boolean_attribute_preserving_previous(app, "AXEnhancedUserInterface", true) {
            Ok(previous) => {
                changes.enhanced_user_interface_restore_value = Some(previous.unwrap_or(false));
            }
            Err(error) if changes.is_empty() => return Err(error),
            Err(_) => {}
        }
    } else {
        manual?;
    }

    Ok(changes)
}

/// Records the activation applied to `pid` so quitting can undo it.
pub fn remember_applied_activation(pid: pid_t, changes: ActivationChanges) {
    if !changes.is_empty() {
        *applied_activation() = Some((pid, changes));
    }
}

/// Restores a boolean AX attribute if a previous value was captured.
fn restore_boolean_attribute(element: &AXUIElement, name: &str, restore_value: Option<bool>) {
    let Some(restore_value) = restore_value else {
        return;
    };

    if let Err(error) = set_boolean_attribute(element, name, restore_value) {
        eprintln!(
            "Unable to restore {name} to {restore_value}: {}",
            error_string(error)
        );
    }
}

/// Sets a boolean AX attribute and returns the previous value when readable.
fn set_boolean_attribute_preserving_previous(
    element: &AXUIElement,
    name: &str,
    value: bool,
) -> Result<Option<bool>, i32> {
    let previous = boolean_attribute_value(element, name).ok();
    set_boolean_attribute(element, name, value)?;

    Ok(previous)
}

/// Reads a boolean AX attribute from an accessibility element.
fn boolean_attribute_value(element: &AXUIElement, name: &str) -> Result<bool, String> {
    let attribute = CFString::new(name);
    let mut value = ptr::null();
    let error = unsafe {
        AXUIElementCopyAttributeValue(
            element.as_concrete_TypeRef(),
            attribute.as_concrete_TypeRef(),
            &mut value,
        )
    };

    if error != kAXErrorSuccess {
        return Err(format!("error:{}", error_string(error)));
    }

    if value.is_null() {
        return Err("null".to_string());
    }

    let value = unsafe { CFType::wrap_under_create_rule(value) };
    if !value.instance_of::<CFBoolean>() {
        return Err("non_boolean".to_string());
    }

    let value = unsafe { CFBoolean::wrap_under_get_rule(value.as_CFTypeRef() as _) };
    Ok(bool::from(value))
}

/// Writes a boolean AX attribute and returns the raw AX error on failure.
fn set_boolean_attribute(element: &AXUIElement, name: &str, value: bool) -> Result<(), i32> {
    let attribute = CFString::new(name);
    let value = if value {
        CFBoolean::true_value()
    } else {
        CFBoolean::false_value()
    };
    let error = unsafe {
        AXUIElementSetAttributeValue(
            element.as_concrete_TypeRef(),
            attribute.as_concrete_TypeRef(),
            value.as_CFTypeRef(),
        )
    };

    if error == kAXErrorSuccess {
        Ok(())
    } else {
        Err(error)
    }
}

/// Walks an app accessibility tree to determine whether activation exposes usable text bounds.
pub fn verify_accessibility_activation(app: &AXUIElement) -> AccessibilityActivationVerification {
    let walker = TreeWalker::new();
    let probe = AccessibilityActivationProbe::new();

    if let Ok(focused) = super::core_foundation_utilities::ax_element_attribute(
        app,
        accessibility_sys::kAXFocusedUIElementAttribute,
    ) {
        walker.walk(&focused, &probe);
        let focused_result = probe.result();
        if focused_result != AccessibilityActivationVerification::NoSupportedTextElement {
            return focused_result;
        }
    }

    walker.walk(app, &probe);

    probe.result()
}

/// Tree visitor used to verify that the activated app exposes usable text bounds.
struct AccessibilityActivationProbe {
    found_supported_text_element: Cell<bool>,
    found_text_range_bounds: Cell<bool>,
    visited: Cell<usize>,
    started: Instant,
}

impl AccessibilityActivationProbe {
    /// Creates an empty activation probe for one accessibility-tree walk.
    fn new() -> Self {
        Self {
            found_supported_text_element: Cell::new(false),
            found_text_range_bounds: Cell::new(false),
            visited: Cell::new(0),
            started: Instant::now(),
        }
    }

    /// Summarizes what the probe found during the tree walk.
    fn result(&self) -> AccessibilityActivationVerification {
        if self.found_text_range_bounds.get() {
            AccessibilityActivationVerification::FoundTextRangeBounds
        } else if self.found_supported_text_element.get() {
            AccessibilityActivationVerification::FoundSupportedTextElement
        } else {
            AccessibilityActivationVerification::NoSupportedTextElement
        }
    }
}

impl TreeVisitor for AccessibilityActivationProbe {
    /// Checks each AX element for supported text and usable range bounds.
    fn enter_element(&self, element: &AXUIElement) -> TreeWalkerFlow {
        let visited = self.visited.get().saturating_add(1);
        self.visited.set(visited);
        if visited > 400 || self.started.elapsed() > Duration::from_millis(100) {
            return TreeWalkerFlow::Exit;
        }

        if let Ok(value) = element.value()
            && is_supported_text_element(element)
        {
            self.found_supported_text_element.set(true);

            let string = cf_type_to_string(&value).unwrap_or_default();
            let bounds_usable = if string.is_empty() {
                false
            } else {
                probe_element_rect_for_text_range(element, 0, 1).has_usable_text_metrics()
                    || element_rect_for_text_range_with_fallback(element, &string, 0, 1).is_some()
            };

            if bounds_usable {
                self.found_text_range_bounds.set(true);
                return TreeWalkerFlow::Exit;
            }
        }

        TreeWalkerFlow::Continue
    }

    fn exit_element(&self, _element: &AXUIElement) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_chromium_browsers_get_enhanced_user_interface() {
        assert!(needs_enhanced_user_interface("com.google.Chrome"));
        assert!(needs_enhanced_user_interface("com.google.Chrome.canary"));
        assert!(needs_enhanced_user_interface("company.thebrowser.Browser"));
        // Electron apps use AXManualAccessibility only.
        assert!(!needs_enhanced_user_interface("com.tinyspeck.slackmacgap"));
        assert!(!needs_enhanced_user_interface("com.microsoft.VSCode"));
        assert!(!needs_enhanced_user_interface("com.apple.TextEdit"));
    }

    #[test]
    fn empty_changes_are_not_remembered() {
        assert!(ActivationChanges::default().is_empty());
        assert!(
            !ActivationChanges {
                manual_accessibility_restore_value: Some(false),
                enhanced_user_interface_restore_value: None,
            }
            .is_empty()
        );
    }

    #[test]
    fn verification_retry_slows_after_fast_attempts() {
        assert_eq!(
            accessibility_activation_verification_retry_interval(
                ACCESSIBILITY_ACTIVATION_FAST_VERIFICATION_ATTEMPTS
            ),
            ACCESSIBILITY_ACTIVATION_VERIFICATION_RETRY_INTERVAL
        );
        assert_eq!(
            accessibility_activation_verification_retry_interval(
                ACCESSIBILITY_ACTIVATION_FAST_VERIFICATION_ATTEMPTS + 1
            ),
            ACCESSIBILITY_ACTIVATION_SLOW_VERIFICATION_RETRY_INTERVAL
        );
    }
}
