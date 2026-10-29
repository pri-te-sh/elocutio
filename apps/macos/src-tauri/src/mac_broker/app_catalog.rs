use cached::cached;
use std::{collections::BTreeMap, path::Path, process::Command, sync::Arc};

use crate::os_broker::AppSearchResult;

const APPLICATION_BUNDLE_CONTENT_TYPE: &str = "com.apple.application-bundle";

pub fn integration_display_name(bundle_id: &str) -> String {
    application_path_for_bundle_id(bundle_id)
        .and_then(|path| display_name_from_app_path(&path))
        .unwrap_or_else(|| bundle_id.to_owned())
}

fn display_name_from_app_path(path: &str) -> Option<String> {
    let file_name = Path::new(path).file_name()?.to_str()?;
    let display_name = file_name.strip_suffix(".app").unwrap_or(file_name).trim();

    if display_name.is_empty() {
        None
    } else {
        Some(display_name.to_string())
    }
}

/// Unfiltered.
///
/// The display name comes from the `.app` path Spotlight already returned, so building the list
/// costs one `mdls` per app rather than an extra `mdfind` per app as well.
///
/// `#[cached]` only caches `Ok` results, so a failed Spotlight query is retried next time.
#[cached]
pub fn installed_application_search_results() -> Result<Arc<Vec<AppSearchResult>>, String> {
    let output = Command::new("mdfind")
        .arg(format!(
            "kMDItemContentType == \"{APPLICATION_BUNDLE_CONTENT_TYPE}\""
        ))
        .output()
        .map_err(|error| format!("Failed to list installed applications: {error}"))?;

    if !output.status.success() {
        return Err("Failed to list installed applications with Spotlight.".to_string());
    }

    let mut by_bundle_id: BTreeMap<String, String> = BTreeMap::new();
    for path in String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
    {
        let Some(bundle_id) = bundle_id_from_app_path(path) else {
            continue;
        };
        let name = display_name_from_app_path(path).unwrap_or_else(|| bundle_id.clone());
        by_bundle_id.entry(bundle_id).or_insert(name);
    }

    Ok(Arc::new(
        by_bundle_id
            .into_iter()
            .map(|(bundle_id, name)| AppSearchResult { name, bundle_id })
            .collect(),
    ))
}

/// Resolves a bundle identifier to an installed `.app` path using Spotlight metadata.
pub fn application_path_for_bundle_id(bundle_id: &str) -> Option<String> {
    let bundle_id = bundle_id.trim();

    if bundle_id.is_empty() {
        return None;
    }

    let predicate_bundle_id = escape_spotlight_string(bundle_id);
    let output = Command::new("mdfind")
        .arg(format!(
            "kMDItemCFBundleIdentifier == \"{predicate_bundle_id}\""
        ))
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && line.ends_with(".app"))
        .map(ToString::to_string)
}

fn bundle_id_from_app_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }

    let output = Command::new("mdls")
        .arg("-raw")
        .arg("-name")
        .arg("kMDItemCFBundleIdentifier")
        .arg(path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let bundle_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if bundle_id.is_empty() || bundle_id == "(null)" {
        None
    } else {
        Some(bundle_id)
    }
}

fn escape_spotlight_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_come_from_the_app_path() {
        assert_eq!(
            display_name_from_app_path("/Applications/Visual Studio Code.app"),
            Some("Visual Studio Code".to_string())
        );
        assert_eq!(display_name_from_app_path("/Applications/.app"), None);
    }

    #[test]
    fn escapes_spotlight_strings() {
        assert_eq!(
            escape_spotlight_string(r#"com.example.\"quoted\""#),
            r#"com.example.\\\"quoted\\\""#
        );
    }

    #[test]
    fn empty_bundle_id_has_no_application_path() {
        assert_eq!(application_path_for_bundle_id(""), None);
        assert_eq!(application_path_for_bundle_id("   "), None);
    }
}
