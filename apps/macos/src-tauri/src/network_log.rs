//! The local network log.
//!
//! The v1.0 definition of done says: "No telemetry or hidden network calls;
//! the network log proves it." A promise like that needs something to point
//! at, so every outbound request is recorded locally: the host, the model,
//! the token counts, the cost, and whether it succeeded.
//!
//! The log records **metadata only**. The text that was sent is not recorded,
//! because a log of what you wrote is a second copy of your writing.
//!
//! The log is what the privacy page means by "every host contacted", and it
//! is how a user can check that nothing left the Mac that they did not ask
//! for.

use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::Config;

fn log_path() -> Result<PathBuf, String> {
    Config::folder_path()
        .map(|folder| folder.join("network-log.jsonl"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

/// One recorded request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkEntry {
    /// Unix seconds.
    pub at: u64,
    /// The host that was contacted, without the path.
    pub host: String,
    /// The provider the host belongs to.
    pub provider: String,
    /// The model, when the request was a model request.
    pub model: Option<String>,
    /// The kind of request.
    pub kind: RequestKind,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
    /// Whether the request succeeded.
    pub ok: bool,
    /// The HTTP status, when there was one.
    pub status: Option<u16>,
    /// The error, when there was one. It is the provider's message, never the
    /// text that was sent.
    pub error: Option<String>,
}

/// What kind of request it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    /// A chat completion: a rewrite or an analysis.
    Chat,
    /// A key-validation request.
    KeyCheck,
    /// A model-list request.
    ModelList,
}

impl RequestKind {
    /// A short label for the settings UI.
    pub const fn label(self) -> &'static str {
        match self {
            RequestKind::Chat => "Chat",
            RequestKind::KeyCheck => "Key check",
            RequestKind::ModelList => "Model list",
        }
    }
}

/// The maximum number of entries kept on disk.
const MAX_ENTRIES: usize = 1_000;

/// Record a request.
///
/// This is called for every outbound request, so the log is a complete record
/// of what left the Mac.
pub fn record(entry: NetworkEntry) -> Result<(), String> {
    let path = log_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the network log directory: {error}"))?;
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Could not open the network log: {error}"))?;
    let line = serde_json::to_string(&entry)
        .map_err(|error| format!("Could not serialize the network entry: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("Could not record the request: {error}"))?;

    trim(&path)?;
    Ok(())
}

/// Record a model request with the usual metadata.
pub fn record_chat(
    host: &str,
    provider: &str,
    model: Option<&str>,
    usage: (u64, u64),
    cost_usd: Option<f64>,
    outcome: Outcome,
) -> Result<(), String> {
    record(NetworkEntry {
        at: now_seconds(),
        host: host.to_string(),
        provider: provider.to_string(),
        model: model.map(str::to_string),
        kind: RequestKind::Chat,
        input_tokens: usage.0,
        output_tokens: usage.1,
        cost_usd,
        ok: outcome.ok,
        status: outcome.status,
        error: outcome.error,
    })
}

/// How a request ended.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub ok: bool,
    pub status: Option<u16>,
    pub error: Option<String>,
}

impl Outcome {
    /// The request succeeded.
    pub fn success(status: u16) -> Self {
        Self {
            ok: true,
            status: Some(status),
            error: None,
        }
    }

    /// The request failed.
    pub fn failure(status: Option<u16>, error: impl Into<String>) -> Self {
        Self {
            ok: false,
            status,
            error: Some(error.into()),
        }
    }
}

/// Read every recorded request.
pub fn read() -> Result<Vec<NetworkEntry>, String> {
    let path = log_path()?;
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Could not read the network log: {error}")),
    };

    Ok(contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

/// A summary of the log, for the settings UI.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkSummary {
    /// How many requests were recorded.
    pub requests: usize,
    /// How many succeeded.
    pub succeeded: usize,
    /// The total tokens sent.
    pub input_tokens: u64,
    /// The total tokens received.
    pub output_tokens: u64,
    /// The total cost.
    pub cost_usd: f64,
    /// Every host that was contacted, with how many times.
    pub hosts: Vec<(String, usize)>,
}

impl NetworkSummary {
    /// Summarize the log.
    pub fn summarize(entries: &[NetworkEntry]) -> Self {
        let mut summary = NetworkSummary {
            requests: entries.len(),
            succeeded: entries.iter().filter(|entry| entry.ok).count(),
            input_tokens: entries.iter().map(|entry| entry.input_tokens).sum(),
            output_tokens: entries.iter().map(|entry| entry.output_tokens).sum(),
            cost_usd: entries.iter().filter_map(|entry| entry.cost_usd).sum(),
            hosts: Vec::new(),
        };

        let mut counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for entry in entries {
            *counts.entry(entry.host.clone()).or_insert(0) += 1;
        }
        summary.hosts = counts.into_iter().collect();
        summary
    }
}

/// Delete the whole log.
pub fn clear() -> Result<(), String> {
    let path = log_path()?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not clear the network log: {error}")),
    }
}

/// Keep the log within [`MAX_ENTRIES`].
fn trim(path: &PathBuf) -> Result<(), String> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Could not read the network log: {error}")),
    };

    let lines: Vec<&str> = contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() <= MAX_ENTRIES {
        return Ok(());
    }

    let kept = &lines[lines.len() - MAX_ENTRIES..];
    let mut file = std::fs::File::create(path)
        .map_err(|error| format!("Could not rewrite the network log: {error}"))?;
    for line in kept {
        writeln!(file, "{line}")
            .map_err(|error| format!("Could not rewrite the network log: {error}"))?;
    }
    Ok(())
}

/// The host of a URL, without the scheme or the path.
pub fn host_of(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .to_string()
}

pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_is_extracted_without_the_scheme_or_path() {
        assert_eq!(host_of("https://openrouter.ai/api/v1/key"), "openrouter.ai");
        assert_eq!(host_of("http://localhost:1234/v1/chat"), "localhost:1234");
        assert_eq!(host_of("openrouter.ai"), "openrouter.ai");
    }

    #[test]
    fn the_summary_counts_the_hosts() {
        let entries = vec![
            NetworkEntry {
                at: 1,
                host: "openrouter.ai".to_string(),
                provider: "OpenRouter".to_string(),
                model: Some("a".to_string()),
                kind: RequestKind::Chat,
                input_tokens: 10,
                output_tokens: 5,
                cost_usd: Some(0.001),
                ok: true,
                status: Some(200),
                error: None,
            },
            NetworkEntry {
                at: 2,
                host: "openrouter.ai".to_string(),
                provider: "OpenRouter".to_string(),
                model: None,
                kind: RequestKind::KeyCheck,
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: None,
                ok: false,
                status: Some(401),
                error: Some("bad key".to_string()),
            },
        ];
        let summary = NetworkSummary::summarize(&entries);
        assert_eq!(summary.requests, 2);
        assert_eq!(summary.succeeded, 1);
        assert_eq!(summary.input_tokens, 10);
        assert_eq!(summary.output_tokens, 5);
        assert_eq!(summary.hosts, vec![("openrouter.ai".to_string(), 2)]);
    }

    #[test]
    fn an_entry_records_no_text() {
        let entry = NetworkEntry {
            at: 1,
            host: "openrouter.ai".to_string(),
            provider: "OpenRouter".to_string(),
            model: Some("anthropic/claude-haiku-5.5".to_string()),
            kind: RequestKind::Chat,
            input_tokens: 10,
            output_tokens: 5,
            cost_usd: Some(0.001),
            ok: true,
            status: Some(200),
            error: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        for forbidden in ["\"text\"", "\"prompt\"", "\"content\"", "\"original\""] {
            assert!(
                !json.contains(forbidden),
                "the log must not record {forbidden}"
            );
        }
    }
}
