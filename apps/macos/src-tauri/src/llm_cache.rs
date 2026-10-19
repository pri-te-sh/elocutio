//! Cache of validated LLM suggestions for the currently focused field.
//!
//! The main process owns providers and analysis; the highlighter owns AX geometry
//! and rendering. They share a small JSON file keyed by pid + text hash rather
//! than sending provider keys or network access into the highlighter.

use std::path::PathBuf;

use elocutio_core::{Category, Resolution, Severity};
use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSuggestion {
    pub id: String,
    pub char_start: usize,
    pub char_end: usize,
    pub original: String,
    pub replacement: String,
    pub category: Category,
    pub severity: Severity,
    pub explanation: String,
    pub resolution: Resolution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedAnalysis {
    pub pid: i32,
    pub bundle_id: String,
    pub text_hash: u64,
    pub model: String,
    pub created_at: u64,
    pub suggestions: Vec<CachedSuggestion>,
}

fn cache_path() -> Result<PathBuf, String> {
    Config::folder_path()
        .map(|folder| folder.join("llm-cache.json"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

/// Small deterministic FNV-1a hash shared by main and highlighter.
pub fn text_hash(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub fn write(cache: &CachedAnalysis) -> Result<(), String> {
    let path = cache_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the cache directory: {error}"))?;
    }
    let temporary = path.with_extension("json.tmp");
    let json = serde_json::to_vec(cache)
        .map_err(|error| format!("Could not serialize suggestions: {error}"))?;
    std::fs::write(&temporary, json)
        .map_err(|error| format!("Could not write the suggestion cache: {error}"))?;
    std::fs::rename(&temporary, &path)
        .map_err(|error| format!("Could not publish the suggestion cache: {error}"))
}

pub fn read() -> Result<Option<CachedAnalysis>, String> {
    let path = cache_path()?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read the suggestion cache: {error}")),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("Invalid suggestion cache: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_changes_with_text() {
        assert_ne!(text_hash("hello"), text_hash("Hello"));
        assert_eq!(text_hash("same"), text_hash("same"));
    }
}
