#![forbid(unsafe_code)]

//! Shared platform traits.
//!
//! Platform shells implement these traits; the analysis engine, provider layer,
//! and voice engine stay platform-agnostic.
//!
//! - macOS: Accessibility + pasteboard/⌘V
//! - Windows: UI Automation + clipboard/Ctrl+V
//! - Android: AccessibilityService/keyboard
//! - iOS: keyboard/share extensions (no cross-app access)
//! - Browser: DOM adapters (separate TypeScript implementation)

use elocutio_core::{Suggestion, TextSpan};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A rectangle in global screen coordinates (platform-defined origin).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ScreenRect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// A focused text field captured from the host platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedField {
    pub bundle_id: String,
    pub role: String,
    pub text: String,
    pub selection: Option<TextSpan>,
}

/// How an edit was applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectMethod {
    Ax,
    Pasteboard,
    AccessibilityAction,
    Keyboard,
}

/// Result of an injection attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InjectReport {
    pub method: InjectMethod,
    pub verified: bool,
}

/// Normalized platform failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlatformError {
    #[error("this app or field is not supported")]
    Unsupported,
    #[error("permission denied")]
    PermissionDenied,
    #[error("focus moved away from the target field")]
    FocusLost,
    #[error("text changed since analysis")]
    StaleText,
    #[error("injection failed: {0}")]
    InjectionFailed(String),
}

/// Read the focused text field, if any.
pub trait TextCapture {
    fn focused_field(&self) -> Result<Option<CapturedField>, PlatformError>;
}

/// Replace a span in the focused field and report whether it was verified.
pub trait TextInjector {
    fn replace(
        &self,
        field: &CapturedField,
        span: TextSpan,
        replacement: &str,
    ) -> Result<InjectReport, PlatformError>;
}

/// Present suggestions near the field without stealing focus.
pub trait OverlayHost {
    fn show_suggestions(
        &self,
        anchor: ScreenRect,
        suggestions: &[Suggestion],
    ) -> Result<(), PlatformError>;
    fn hide(&self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_rect_center_is_computed() {
        let rect = ScreenRect::new(10.0, 20.0, 100.0, 40.0);
        assert_eq!(rect.center(), (60.0, 40.0));
    }

    #[test]
    fn platform_errors_have_user_readable_messages() {
        assert_eq!(
            PlatformError::StaleText.to_string(),
            "text changed since analysis"
        );
    }

    #[test]
    fn inject_methods_serialize_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&InjectMethod::Pasteboard).unwrap(),
            "\"pasteboard\""
        );
    }
}
