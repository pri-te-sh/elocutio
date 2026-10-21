//! The app support matrix, generated from `app_matrix.json` at build time.
//!
//! The overlay allowlist lives here rather than in the broker so the tracked
//! matrix and the shipped behavior are the same list. `docs/M0-VERIFICATION.md`
//! and the progress ledger both read the JSON, so a change here shows up in the
//! tracker instead of silently narrowing coverage.

use std::collections::BTreeSet;

/// The apps the M0 gate requires the overlay to work in.
pub const TARGET_OVERLAY_APPS: &[&str] = &[
    "com.apple.TextEdit",
    "com.apple.Notes",
    "com.apple.mail",
    "com.apple.Safari",
    "com.google.Chrome",
];

/// The apps the overlay currently draws in.
///
/// This is deliberately smaller than [`TARGET_OVERLAY_APPS`] until the Chrome
/// extension (M3) and the Mail text-bounds fallback land. Probing Chromium and
/// Mail Accessibility trees can raise a foreign exception and abort the
/// highlighter, so both are excluded by policy rather than by accident.
pub const OVERLAY_SUPPORTED: &[&str] =
    &["com.apple.TextEdit", "com.apple.Notes", "com.apple.Safari"];

/// The apps where the pasteboard write ladder has been verified interactively.
pub const WRITE_LADDER_VERIFIED: &[&str] = &[
    "com.apple.TextEdit",
    "com.apple.Safari",
    "com.google.Chrome",
];

/// Whether the overlay draws in the given bundle identifier.
pub fn overlay_supported(bundle_id: &str) -> bool {
    OVERLAY_SUPPORTED.contains(&bundle_id)
}

/// The set of target apps the overlay does not yet cover.
pub fn missing_overlay_apps() -> BTreeSet<&'static str> {
    TARGET_OVERLAY_APPS
        .iter()
        .copied()
        .filter(|bundle_id| !overlay_supported(bundle_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matrix_lists_five_target_apps() {
        assert_eq!(TARGET_OVERLAY_APPS.len(), 5);
    }

    #[test]
    fn every_overlay_app_is_a_target_app() {
        for bundle_id in OVERLAY_SUPPORTED {
            assert!(
                TARGET_OVERLAY_APPS.contains(bundle_id),
                "{bundle_id} is not a target app"
            );
        }
    }

    #[test]
    fn the_missing_apps_are_chrome_and_mail() {
        let missing = missing_overlay_apps();
        assert!(missing.contains("com.google.Chrome"));
        assert!(missing.contains("com.apple.mail"));
    }

    #[test]
    fn overlay_support_matches_the_list() {
        assert!(overlay_supported("com.apple.TextEdit"));
        assert!(overlay_supported("com.apple.Notes"));
        assert!(overlay_supported("com.apple.Safari"));
        assert!(!overlay_supported("com.google.Chrome"));
        assert!(!overlay_supported("com.apple.mail"));
    }
}
