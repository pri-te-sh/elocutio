use accessibility::ui_element::AXUIElement;
use accessibility_sys::{
    AXUIElementGetPid, error_string, kAXErrorSuccess, kAXFocusedApplicationAttribute, pid_t,
};
use core_foundation::base::TCFType;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use std::error::Error as StdError;

use super::BUNDLE_ID;
use super::core_foundation_utilities::ax_element_attribute;

/// Returns whether a running application belongs to Elocutio (bundled or unbundled dev binary).
pub fn is_elocutio_running_app(app: &NSRunningApplication) -> bool {
    let pid = app.processIdentifier();
    if pid == std::process::id() as pid_t {
        return true;
    }
    if let Some(bundle_id) = app.bundleIdentifier()
        && bundle_id.to_string() == BUNDLE_ID
    {
        return true;
    }
    if let Some(name) = app.localizedName() {
        let name = name.to_string();
        if name.eq_ignore_ascii_case("elocutio") || name.eq_ignore_ascii_case("elocutio-desktop") {
            return true;
        }
    }
    if let Some(url) = app.executableURL()
        && let Some(path) = url.path()
    {
        let path = path.to_string();
        if path.ends_with("/elocutio-desktop") || path.ends_with("/Elocutio") {
            return true;
        }
    }
    false
}

/// Returns whether a PID belongs to Elocutio (bundled or unbundled dev binary).
pub fn is_elocutio_pid(pid: pid_t) -> bool {
    if pid == std::process::id() as pid_t {
        return true;
    }
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .is_some_and(|app| is_elocutio_running_app(&app))
}

/// Pretty self-explanatory. Grabs the PID of the currently focused window.
/// Can be expensive. Do not perform in a hot loop.
pub fn focused_window_pid() -> Result<pid_t, Box<dyn StdError>> {
    let system = AXUIElement::system_wide();
    let app = match ax_element_attribute(&system, kAXFocusedApplicationAttribute) {
        Ok(app) => app,
        Err(error) => {
            if let Some(pid) = frontmost_application_pid() {
                return Ok(pid);
            }

            return Err(error);
        }
    };

    let mut pid: pid_t = 0;
    let err = unsafe { AXUIElementGetPid(app.as_concrete_TypeRef(), &mut pid) };

    if err != kAXErrorSuccess {
        if let Some(pid) = frontmost_application_pid() {
            return Ok(pid);
        }

        return Err(format!("AXUIElementGetPid failed: {}", error_string(err)).into());
    }

    Ok(pid)
}

/// Fallback PID lookup when system-wide focused-application AX lookup fails.
fn frontmost_application_pid() -> Option<pid_t> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
}

/// The PID of the application whose text should be read.
///
/// Clicking on `"elo-overlay"` must never clear `MacBroker`'s remembered target (`last_focused`).
/// Only when the user is actively focused in the `Elocutio — Writing Studio & Settings` window
/// (`label = "settings"`) should the system-wide overlay hide.
pub fn focused_text_pid() -> Result<pid_t, Box<dyn StdError>> {
    if crate::elo_overlay::is_studio_window_focused() {
        return Err("Elocutio Studio window is focused".into());
    }
    let pid = focused_window_pid()?;
    Ok(pid)
}
