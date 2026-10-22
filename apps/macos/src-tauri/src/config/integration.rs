use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Integration {
    pub bundle_id: String,
    pub enabled: bool,
}

impl Integration {
    pub fn curated_integrations() -> Vec<Self> {
        #[cfg(target_os = "macos")]
        let integrations = [
            "com.apple.TextEdit",
            "com.apple.mail",
            "com.apple.MobileSMS",
            "com.apple.Notes",
            "com.apple.Safari",
            "com.google.Chrome",
            "com.brave.Browser",
            "com.microsoft.edgemac",
            "company.thebrowser.Browser",
            "org.mozilla.firefox",
            "com.tinyspeck.slackmacgap",
            "com.hnc.Discord",
            "com.bloombuilt.dayone-mac",
            "com.microsoft.Word",
            "com.microsoft.Outlook",
            "com.microsoft.teams2",
            "com.microsoft.teams",
            "com.microsoft.VSCode",
            "com.google.antigravity",
            "com.anthropic.claudefordesktop",
            "com.deepseek.dsh",
            "notion.id",
            "md.obsidian",
            "com.linear",
        ];

        #[cfg(target_os = "windows")]
        let integrations: [&str; 0] = [];

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let integrations: [&str; 0] = [];

        integrations
            .into_iter()
            .map(|bundle_id| Integration {
                bundle_id: bundle_id.to_string(),
                enabled: true,
            })
            .collect()
    }
}
