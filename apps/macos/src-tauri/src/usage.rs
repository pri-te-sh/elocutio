//! Local-only usage log.
//!
//! Records request metadata (model, tokens, optional cost) in
//! `~/Library/Application Support/elocutio/usage.jsonl`. It never records the
//! selected text or the rewritten text.

use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::openrouter::RewriteResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEntry {
    pub timestamp: u64,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageSummary {
    pub requests: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub recent: Vec<UsageEntry>,
}

fn log_path() -> Result<PathBuf, String> {
    Config::folder_path()
        .map(|folder| folder.join("usage.jsonl"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

pub fn record(result: &RewriteResult) -> Result<(), String> {
    let path = log_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the usage directory: {error}"))?;
    }

    let entry = UsageEntry {
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("Clock error: {error}"))?
            .as_secs(),
        model: result.model.clone(),
        input_tokens: result.input_tokens,
        output_tokens: result.output_tokens,
        cost_usd: result.cost_usd,
    };

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("Could not open the usage log: {error}"))?;
    let line = serde_json::to_string(&entry)
        .map_err(|error| format!("Could not serialize usage: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("Could not append usage: {error}"))
}

pub fn summary(limit: usize) -> Result<UsageSummary, String> {
    let path = log_path()?;
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UsageSummary {
                requests: 0,
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: 0.0,
                recent: Vec::new(),
            });
        }
        Err(error) => return Err(format!("Could not read the usage log: {error}")),
    };

    let mut entries = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| format!("Could not read the usage log: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<UsageEntry>(&line) {
            entries.push(entry);
        }
    }

    Ok(summarize(entries, limit))
}

fn summarize(entries: Vec<UsageEntry>, limit: usize) -> UsageSummary {
    let requests = entries.len();
    let input_tokens = entries.iter().map(|entry| entry.input_tokens).sum();
    let output_tokens = entries.iter().map(|entry| entry.output_tokens).sum();
    let cost_usd = entries.iter().filter_map(|entry| entry.cost_usd).sum();
    let recent = entries.into_iter().rev().take(limit).collect::<Vec<_>>();

    UsageSummary {
        requests,
        input_tokens,
        output_tokens,
        cost_usd,
        recent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(timestamp: u64, input: u64, output: u64, cost: Option<f64>) -> UsageEntry {
        UsageEntry {
            timestamp,
            model: "anthropic/claude-haiku-5.5".to_string(),
            input_tokens: input,
            output_tokens: output,
            cost_usd: cost,
        }
    }

    #[test]
    fn summarizes_tokens_and_cost() {
        let summary = summarize(
            vec![entry(1, 10, 20, Some(0.001)), entry(2, 30, 40, Some(0.002))],
            10,
        );

        assert_eq!(summary.requests, 2);
        assert_eq!(summary.input_tokens, 40);
        assert_eq!(summary.output_tokens, 60);
        assert!((summary.cost_usd - 0.003).abs() < f64::EPSILON);
        assert_eq!(summary.recent.len(), 2);
        assert_eq!(summary.recent[0].timestamp, 2);
    }

    #[test]
    fn handles_missing_cost() {
        let summary = summarize(vec![entry(1, 10, 20, None)], 10);
        assert_eq!(summary.cost_usd, 0.0);
    }
}
