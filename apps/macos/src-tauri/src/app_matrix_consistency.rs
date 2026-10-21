//! Checks that the Rust overlay allowlist and the tracked JSON matrix agree.
//!
//! The matrix is tracked in `src/app_matrix.json` because the progress ledger
//! reads it. If the Rust allowlist drifts from the JSON, the tracker would
//! report a matrix the app does not implement, so this test fails instead.

#[cfg(test)]
const MATRIX_JSON: &str = include_str!("app_matrix.json");

/// A tiny reader for the three lists the test needs.
///
/// The file is generated and reviewed by hand, so a focused reader is enough;
/// it avoids pulling a JSON dependency into the desktop binary for one test.
#[cfg(test)]
fn json_list(key: &str) -> Vec<String> {
    let needle = format!("\"{key}\"");
    let start = MATRIX_JSON
        .find(&needle)
        .unwrap_or_else(|| panic!("{key} is missing from app_matrix.json"));
    let rest = &MATRIX_JSON[start..];
    let open = rest
        .find('[')
        .unwrap_or_else(|| panic!("{key} has no array in app_matrix.json"));
    let close = rest[open..]
        .find(']')
        .unwrap_or_else(|| panic!("{key} has no closing bracket in app_matrix.json"));
    rest[open + 1..open + close]
        .split(',')
        .map(|value| value.trim().trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::json_list;
    use crate::app_matrix::{OVERLAY_SUPPORTED, TARGET_OVERLAY_APPS, WRITE_LADDER_VERIFIED};

    #[test]
    fn the_json_matrix_matches_the_rust_allowlist() {
        let json_supported = json_list("overlaySupported");
        assert_eq!(
            json_supported,
            OVERLAY_SUPPORTED
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>(),
            "app_matrix.json and app_matrix.rs disagree about the overlay allowlist"
        );
    }

    #[test]
    fn the_json_target_list_matches_the_rust_target_list() {
        let json_targets = json_list("targetOverlayApps");
        assert_eq!(
            json_targets,
            TARGET_OVERLAY_APPS
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>(),
            "app_matrix.json and app_matrix.rs disagree about the target apps"
        );
    }

    #[test]
    fn the_json_write_ladder_list_matches_the_rust_list() {
        let json_verified = json_list("writeLadderVerified");
        assert_eq!(
            json_verified,
            WRITE_LADDER_VERIFIED
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>(),
            "app_matrix.json and app_matrix.rs disagree about the write-ladder list"
        );
    }

    #[test]
    fn the_target_list_is_the_five_app_m0_gate() {
        assert_eq!(
            TARGET_OVERLAY_APPS.len(),
            5,
            "the M0 gate is an overlay in five apps"
        );
    }
}
