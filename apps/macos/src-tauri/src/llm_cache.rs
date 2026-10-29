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

pub fn cache_path() -> Result<PathBuf, String> {
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
    // A unique temporary name per write, so two analyses finishing at once
    // never write into (or rename away) each other's temporary file.
    static WRITE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = WRITE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("json.{}.{seq}.tmp", std::process::id()));
    let json = serde_json::to_vec(cache)
        .map_err(|error| format!("Could not serialize suggestions: {error}"))?;
    std::fs::write(&temporary, json)
        .map_err(|error| format!("Could not write the suggestion cache: {error}"))?;
    std::fs::rename(&temporary, &path)
        .map_err(|error| format!("Could not publish the suggestion cache: {error}"))
}

/// How long a cached analysis stays valid.
///
/// The suggestions in the cache were computed for one specific text, and the
/// cache is keyed by that text's hash, so a stale entry can never be shown
/// against different text. The TTL matters for a different reason: the file
/// holds the user's text-derived suggestions on disk, and it should not sit
/// there indefinitely.
const CACHE_TTL_SECONDS: u64 = 24 * 60 * 60;

/// Whether a cached analysis is still within its time to live.
pub fn is_fresh(cache: &CachedAnalysis, now: u64) -> bool {
    now.saturating_sub(cache.created_at) < CACHE_TTL_SECONDS
}

/// The current unix time in seconds.
pub fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn read() -> Result<Option<CachedAnalysis>, String> {
    let path = cache_path()?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read the suggestion cache: {error}")),
    };
    let cache: CachedAnalysis = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid suggestion cache: {error}"))?;

    if !is_fresh(&cache, now_seconds()) {
        // The cache is stale, so drop it rather than keeping the user's
        // derived text on disk.
        let _ = std::fs::remove_file(&path);
        return Ok(None);
    }

    Ok(Some(cache))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_changes_with_text() {
        assert_ne!(text_hash("hello"), text_hash("Hello"));
        assert_eq!(text_hash("same"), text_hash("same"));
    }

    #[test]
    fn a_fresh_cache_is_fresh() {
        let cache = CachedAnalysis {
            pid: 1,
            bundle_id: "com.example.Editor".to_string(),
            text_hash: 1,
            model: "model".to_string(),
            created_at: 1_000,
            suggestions: Vec::new(),
        };
        assert!(is_fresh(&cache, 1_000));
        assert!(is_fresh(&cache, 1_000 + CACHE_TTL_SECONDS - 1));
    }

    #[test]
    fn an_old_cache_is_stale() {
        let cache = CachedAnalysis {
            pid: 1,
            bundle_id: "com.example.Editor".to_string(),
            text_hash: 1,
            model: "model".to_string(),
            created_at: 1_000,
            suggestions: Vec::new(),
        };
        assert!(!is_fresh(&cache, 1_000 + CACHE_TTL_SECONDS));
        assert!(!is_fresh(&cache, 1_000 + CACHE_TTL_SECONDS * 10));
    }
}
