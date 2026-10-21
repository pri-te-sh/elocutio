use harper_core::linting::Lint;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::rect::ActionableLint;

pub type LintText = Box<dyn FnMut(&str) -> BTreeMap<String, Vec<Lint>>>;

#[derive(Debug, Clone, Serialize)]
pub struct FocusedText {
    pub pid: i32,
    pub bundle_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AccessibilityPermissionStatus {
    Granted,
    NotGranted,
    Unsupported,
}

/// Provides platform-specific state needed by the highlighter without coupling rendering to an OS.
///
/// The highlighter needs both accessibility-derived lint rectangles and global cursor position, but
/// those APIs are platform-specific. This trait keeps the event loop and renderer independent from
/// macOS accessibility and pointer APIs.
pub trait OsBroker {
    /// Identifies Elocutio for automatic-discovery exclusion, not for manual integration policy.
    ///
    /// Uses the configured bundle ID on macOS and the current executable path on Windows.
    /// Unsupported platforms return `false`. No broker instance is needed; `Self: Sized` keeps
    /// the trait usable as `dyn OsBroker`.
    fn is_elocutio(_app_id: &str) -> bool
    where
        Self: Sized,
    {
        false
    }

    /// Get the actionable lint boxes from the OS, provided a linting source.
    ///
    /// `None` means the accessibility read failed and the last successful result should be retained.
    fn get_boxes(
        &mut self,
        lint_text: &mut dyn FnMut(&str) -> BTreeMap<String, Vec<Lint>>,
    ) -> Option<Vec<ActionableLint>>;

    /// Grab the position of the user's cursor on the screen.
    fn cursor_position(&self) -> Option<egui::Pos2>;

    /// Check whether Elocutio has permission to access the OS' native accessibility API.
    fn accessibility_permission_status(&self) -> AccessibilityPermissionStatus;

    /// Request permission to access the OS' native accessibility API.
    fn request_accessibility_permission(&self) -> AccessibilityPermissionStatus;

    /// Given an application identifier, find that integration's human-readable name.
    fn integration_display_name(&self, bundle_id: &str) -> String {
        bundle_id.to_owned()
    }

    /// Returns the bundle identifiers for installed graphical applications.
    ///
    /// Implementations should return stable bundle ID strings, sorted and deduplicated where
    /// possible. Platforms that do not support bundle IDs should return an error.
    fn installed_application_bundle_ids(&self) -> Result<Vec<String>, String>;

    /// Returns the application icon for `bundle_id` encoded as PNG bytes.
    ///
    /// The broker returns raw bytes so callers can choose their own transport format, such as a
    /// Tauri command converting them into a data URL.
    fn application_icon_png(&self, _bundle_id: &str) -> Result<Vec<u8>, String>;

    /// Start an application given its bundle ID.
    fn launch_app_bundle(&self, _bundle_id: &str) -> Result<(), String>;

    /// Search for an application in the OS' global list of installed apps.
    fn search_apps(&self, _query: &str) -> Result<Vec<AppSearchResult>, String>;

    /// Read the current selection from the focused text field, if any.
    ///
    /// This is used by the explicit "Rewrite Selection" action in the main
    /// process. The highlighter keeps its own path for overlay geometry.
    fn selected_text(&mut self) -> Result<Option<String>, String> {
        Ok(None)
    }

    /// Replace the current selection by putting `replacement` on the pasteboard
    /// and synthesizing the platform paste shortcut. Returns whether the
    /// replacement was observed in the field afterwards.
    fn replace_selection_via_paste(&mut self, _replacement: &str) -> Result<bool, String> {
        Err("This platform does not support paste replacement yet.".into())
    }

    /// Read the full value of the focused editable field for explicit analysis.
    fn focused_text(&mut self) -> Result<Option<FocusedText>, String> {
        Ok(None)
    }

    /// The tone detected for the focused field, if it was clear enough to name.
    ///
    /// The detection is local and deterministic.
    fn detected_tone(&self) -> Option<String> {
        None
    }

    /// The bundle identifier of the app in front, if it can be determined.
    ///
    /// The highlighter needs this to ask for the settings of the app the user
    /// is actually looking at.
    fn focused_bundle_id(&mut self) -> Result<Option<String>, String> {
        Ok(None)
    }

    /// Replace the full value of the focused editable field for undo.
    fn replace_focused_text(&mut self, _text: &str) -> Result<bool, String> {
        Err("This platform does not support field undo yet.".into())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AppSearchResult {
    pub name: String,
    pub bundle_id: String,
}

/// No-op platform broker for targets that do not have an OS implementation yet.
///
/// This lets the highlighter compile on non-macOS platforms while making it explicit that there is
/// currently no accessibility or cursor integration there.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[derive(Default)]
pub struct NoopBroker;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl OsBroker for NoopBroker {
    fn get_boxes(
        &mut self,
        _lint_text: &mut dyn FnMut(&str) -> BTreeMap<String, Vec<Lint>>,
    ) -> Option<Vec<ActionableLint>> {
        Some(Vec::new())
    }

    fn cursor_position(&self) -> Option<egui::Pos2> {
        None
    }

    fn accessibility_permission_status(&self) -> AccessibilityPermissionStatus {
        AccessibilityPermissionStatus::Unsupported
    }

    fn request_accessibility_permission(&self) -> AccessibilityPermissionStatus {
        AccessibilityPermissionStatus::Unsupported
    }

    fn installed_application_bundle_ids(&self) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }

    fn application_icon_png(&self, _bundle_id: &str) -> Result<Vec<u8>, String> {
        Err("Cannot get application icons.".to_string())
    }

    fn launch_app_bundle(&self, _bundle_id: &str) -> Result<(), String> {
        Ok(())
    }

    fn search_apps(&self, _query: &str) -> Result<Vec<AppSearchResult>, String> {
        Ok(Vec::new())
    }
}
