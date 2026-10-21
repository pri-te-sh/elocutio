//! The Tier A app matrix.
//!
//! The plan's M2 acceptance is a "Tier A matrix: 10 apps across light/dark,
//! multi-monitor, Spaces, and full-screen", and the v1.0 definition of done
//! says "12 Tier A apps pass the manual matrix on current and previous
//! macOS".
//!
//! A manual matrix cannot be automated, because it needs a human looking at
//! a screen. What *can* be automated is the part that goes wrong silently:
//! the list of apps, the conditions to check, and a record of what has
//! actually been verified. This module holds that list and the record, so
//! the matrix is a thing you can run rather than a thing you remember.
//!
//! The record is explicit about what is and is not verified. An app that has
//! never been checked is **unverified**, not "passing".

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// An app in the matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixApp {
    /// The bundle identifier.
    pub bundle_id: &'static str,
    /// The human-readable name.
    pub name: &'static str,
    /// Which tier the plan puts it in.
    pub tier: Tier,
}

/// The plan's app tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// In the first release.
    A,
    /// Supported, but not in the first release.
    B,
    /// Explicitly not supported.
    Unsupported,
}

/// The conditions the matrix has to be checked under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    LightAppearance,
    DarkAppearance,
    Retina,
    NonRetina,
    TwoMonitors,
    Scaling,
    Spaces,
    FullScreen,
    StageManager,
    IncreasedContrast,
    ReducedMotion,
    SecureField,
    PasswordManager,
    Terminal,
    ScreenSharing,
    Offline,
    SlowNetwork,
    Vpn,
    FreshInstall,
    PermissionRevoked,
    SecondUser,
}

/// Every condition, for the checklist.
pub const ALL_CONDITIONS: [Condition; 21] = [
    Condition::LightAppearance,
    Condition::DarkAppearance,
    Condition::Retina,
    Condition::NonRetina,
    Condition::TwoMonitors,
    Condition::Scaling,
    Condition::Spaces,
    Condition::FullScreen,
    Condition::StageManager,
    Condition::IncreasedContrast,
    Condition::ReducedMotion,
    Condition::SecureField,
    Condition::PasswordManager,
    Condition::Terminal,
    Condition::ScreenSharing,
    Condition::Offline,
    Condition::SlowNetwork,
    Condition::Vpn,
    Condition::FreshInstall,
    Condition::PermissionRevoked,
    Condition::SecondUser,
];

impl Condition {
    /// A short label for the checklist.
    pub const fn label(self) -> &'static str {
        match self {
            Condition::LightAppearance => "Light appearance",
            Condition::DarkAppearance => "Dark appearance",
            Condition::Retina => "Retina",
            Condition::NonRetina => "Non-Retina",
            Condition::TwoMonitors => "Two monitors",
            Condition::Scaling => "Display scaling",
            Condition::Spaces => "Spaces",
            Condition::FullScreen => "Full screen",
            Condition::StageManager => "Stage Manager",
            Condition::IncreasedContrast => "Increased contrast",
            Condition::ReducedMotion => "Reduced motion",
            Condition::SecureField => "Secure field",
            Condition::PasswordManager => "Password manager",
            Condition::Terminal => "Terminal",
            Condition::ScreenSharing => "Screen sharing",
            Condition::Offline => "Offline",
            Condition::SlowNetwork => "Slow network",
            Condition::Vpn => "VPN",
            Condition::FreshInstall => "Fresh install",
            Condition::PermissionRevoked => "Permission revoked mid-session",
            Condition::SecondUser => "Second user",
        }
    }
}

/// What has been checked for one app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    /// Never checked.
    Unverified,
    /// Checked and it worked.
    Pass,
    /// Checked and it did not work.
    Fail,
}

/// The record of what has been verified.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MatrixRecord {
    /// App bundle identifier -> the conditions that were checked.
    checked: BTreeMap<String, BTreeMap<String, CheckState>>,
}

impl MatrixRecord {
    /// The state of one condition for one app.
    pub fn state(&self, bundle_id: &str, condition: Condition) -> CheckState {
        self.checked
            .get(bundle_id)
            .and_then(|conditions| conditions.get(condition_key(condition)))
            .copied()
            .unwrap_or(CheckState::Unverified)
    }

    /// Record the result of a check.
    pub fn record(&mut self, bundle_id: &str, condition: Condition, state: CheckState) {
        self.checked
            .entry(bundle_id.to_string())
            .or_default()
            .insert(condition_key(condition).to_string(), state);
    }

    /// How many conditions have been checked for an app.
    pub fn checked_count(&self, bundle_id: &str) -> usize {
        self.checked
            .get(bundle_id)
            .map(|conditions| conditions.len())
            .unwrap_or(0)
    }

    /// How many conditions have failed for an app.
    pub fn failed_count(&self, bundle_id: &str) -> usize {
        self.checked
            .get(bundle_id)
            .map(|conditions| {
                conditions
                    .values()
                    .filter(|state| **state == CheckState::Fail)
                    .count()
            })
            .unwrap_or(0)
    }

    /// Forget every record.
    pub fn clear(&mut self) {
        self.checked.clear();
    }
}

/// The stable key for a condition.
pub fn condition_key(condition: Condition) -> &'static str {
    match condition {
        Condition::LightAppearance => "light_appearance",
        Condition::DarkAppearance => "dark_appearance",
        Condition::Retina => "retina",
        Condition::NonRetina => "non_retina",
        Condition::TwoMonitors => "two_monitors",
        Condition::Scaling => "scaling",
        Condition::Spaces => "spaces",
        Condition::FullScreen => "full_screen",
        Condition::StageManager => "stage_manager",
        Condition::IncreasedContrast => "increased_contrast",
        Condition::ReducedMotion => "reduced_motion",
        Condition::SecureField => "secure_field",
        Condition::PasswordManager => "password_manager",
        Condition::Terminal => "terminal",
        Condition::ScreenSharing => "screen_sharing",
        Condition::Offline => "offline",
        Condition::SlowNetwork => "slow_network",
        Condition::Vpn => "vpn",
        Condition::FreshInstall => "fresh_install",
        Condition::PermissionRevoked => "permission_revoked",
        Condition::SecondUser => "second_user",
    }
}

/// The condition with the given stable key, if there is one.
pub fn condition_from_key(key: &str) -> Option<Condition> {
    ALL_CONDITIONS
        .into_iter()
        .find(|condition| condition_key(*condition) == key)
}

/// Where the matrix record is stored.
fn record_path() -> Result<std::path::PathBuf, String> {
    crate::config::Config::folder_path()
        .map(|folder| folder.join("app-matrix.json"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

/// Read the matrix record.
pub fn load_record() -> Result<MatrixRecord, String> {
    let path = record_path()?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MatrixRecord::default());
        }
        Err(error) => return Err(format!("Could not read the app matrix record: {error}")),
    };
    serde_json::from_slice(&bytes).map_err(|error| format!("Invalid app matrix record: {error}"))
}

/// Save the matrix record.
pub fn save_record(record: &MatrixRecord) -> Result<(), String> {
    let path = record_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the app matrix directory: {error}"))?;
    }
    let json = serde_json::to_vec(record)
        .map_err(|error| format!("Could not serialize the app matrix record: {error}"))?;
    std::fs::write(&path, json)
        .map_err(|error| format!("Could not write the app matrix record: {error}"))
}

/// The Tier A apps the plan lists.
///
/// The plan names twelve Tier A apps; the matrix records which of them the
/// overlay actually draws in, which is a smaller set.
pub const TIER_A_APPS: &[MatrixApp] = &[
    MatrixApp {
        bundle_id: "com.apple.TextEdit",
        name: "TextEdit",
        tier: Tier::A,
    },
    MatrixApp {
        bundle_id: "com.apple.Notes",
        name: "Notes",
        tier: Tier::A,
    },
    MatrixApp {
        bundle_id: "com.apple.mail",
        name: "Mail",
        tier: Tier::A,
    },
    MatrixApp {
        bundle_id: "com.apple.MobileSMS",
        name: "Messages",
        tier: Tier::A,
    },
    MatrixApp {
        bundle_id: "com.apple.Safari",
        name: "Safari",
        tier: Tier::A,
    },
    MatrixApp {
        bundle_id: "com.google.Chrome",
        name: "Chrome",
        tier: Tier::A,
    },
    MatrixApp {
        bundle_id: "com.apple.iWork.Pages",
        name: "Pages",
        tier: Tier::A,
    },
    MatrixApp {
        bundle_id: "com.microsoft.Word",
        name: "Microsoft Word",
        tier: Tier::A,
    },
    MatrixApp {
        bundle_id: "com.tinyspeck.slackmacgap",
        name: "Slack",
        tier: Tier::A,
    },
    MatrixApp {
        bundle_id: "md.obsidian",
        name: "Obsidian",
        tier: Tier::A,
    },
    MatrixApp {
        bundle_id: "com.google.Gmail",
        name: "Gmail (extension)",
        tier: Tier::A,
    },
];

/// The Tier B apps the plan lists.
pub const TIER_B_APPS: &[MatrixApp] = &[
    MatrixApp {
        bundle_id: "notion.id",
        name: "Notion",
        tier: Tier::B,
    },
    MatrixApp {
        bundle_id: "com.linear",
        name: "Linear",
        tier: Tier::B,
    },
    MatrixApp {
        bundle_id: "com.atlassian.jira",
        name: "Jira",
        tier: Tier::B,
    },
    MatrixApp {
        bundle_id: "com.microsoft.Outlook",
        name: "Outlook",
        tier: Tier::B,
    },
    MatrixApp {
        bundle_id: "com.microsoft.VSCode",
        name: "VS Code",
        tier: Tier::B,
    },
    MatrixApp {
        bundle_id: "com.microsoft.excel",
        name: "Excel",
        tier: Tier::B,
    },
    MatrixApp {
        bundle_id: "com.microsoft.Powerpoint",
        name: "PowerPoint",
        tier: Tier::B,
    },
];

/// The apps the plan says are explicitly unsupported.
pub const UNSUPPORTED_APPS: &[(&str, &str)] = &[
    (
        "com.google.docs",
        "Google Docs — canvas rendering, no DOM or AX text",
    ),
    (
        "com.apple.Terminal",
        "Terminal — reading a terminal is not a writing task",
    ),
    (
        "com.microsoft.teams2",
        "Microsoft Teams — no usable text element",
    ),
];

/// A summary of the matrix.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixSummary {
    /// How many Tier A apps the plan lists.
    pub tier_a: usize,
    /// How many Tier A apps the overlay draws in.
    pub supported: usize,
    /// How many conditions have been checked in total.
    pub checked: usize,
    /// How many checks failed.
    pub failed: usize,
    /// How many checks are still unverified.
    pub unverified: usize,
    /// The apps that have at least one failing check.
    pub failing_apps: Vec<String>,
}

impl MatrixSummary {
    /// Summarize the matrix against a record.
    pub fn summarize(record: &MatrixRecord) -> Self {
        let mut summary = MatrixSummary {
            tier_a: TIER_A_APPS.len(),
            ..Default::default()
        };

        for app in TIER_A_APPS {
            if crate::app_matrix::overlay_supported(app.bundle_id) {
                summary.supported += 1;
            }
        }

        for app in TIER_A_APPS {
            let mut app_failed = false;
            for condition in ALL_CONDITIONS {
                match record.state(app.bundle_id, condition) {
                    CheckState::Pass => summary.checked += 1,
                    CheckState::Fail => {
                        summary.checked += 1;
                        summary.failed += 1;
                        app_failed = true;
                    }
                    CheckState::Unverified => summary.unverified += 1,
                }
            }
            if app_failed {
                summary.failing_apps.push(app.name.to_string());
            }
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matrix_lists_the_apps_the_plan_names() {
        // The plan's tier table names eleven Tier A apps, while its
        // definition of done says twelve. The matrix records the eleven the
        // table names; the discrepancy is documented rather than papered
        // over by inventing a twelfth.
        assert_eq!(TIER_A_APPS.len(), 11);
    }

    #[test]
    fn every_tier_a_app_has_a_unique_bundle_id() {
        let mut ids: Vec<&str> = TIER_A_APPS.iter().map(|app| app.bundle_id).collect();
        ids.sort();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "a bundle id appears twice");
    }

    #[test]
    fn an_unchecked_condition_is_unverified_not_passing() {
        let record = MatrixRecord::default();
        assert_eq!(
            record.state("com.apple.TextEdit", Condition::DarkAppearance),
            CheckState::Unverified
        );
        assert_eq!(record.checked_count("com.apple.TextEdit"), 0);
    }

    #[test]
    fn a_check_is_recorded() {
        let mut record = MatrixRecord::default();
        record.record(
            "com.apple.TextEdit",
            Condition::DarkAppearance,
            CheckState::Pass,
        );
        assert_eq!(
            record.state("com.apple.TextEdit", Condition::DarkAppearance),
            CheckState::Pass
        );
        assert_eq!(record.checked_count("com.apple.TextEdit"), 1);
    }

    #[test]
    fn the_summary_counts_what_is_unchecked() {
        let mut record = MatrixRecord::default();
        record.record(
            "com.apple.TextEdit",
            Condition::LightAppearance,
            CheckState::Pass,
        );
        record.record(
            "com.apple.TextEdit",
            Condition::DarkAppearance,
            CheckState::Fail,
        );
        let summary = MatrixSummary::summarize(&record);
        assert_eq!(summary.checked, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(
            summary.unverified,
            TIER_A_APPS.len() * ALL_CONDITIONS.len() - 2
        );
        assert!(summary.failing_apps.contains(&"TextEdit".to_string()));
    }

    #[test]
    fn the_record_round_trips_through_json() {
        let mut record = MatrixRecord::default();
        record.record("com.apple.TextEdit", Condition::Spaces, CheckState::Pass);
        let json = serde_json::to_string(&record).unwrap();
        let decoded: MatrixRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, record);
    }
}
