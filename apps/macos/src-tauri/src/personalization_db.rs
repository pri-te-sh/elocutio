//! Local SQLite personalization & encrypted credentials database (`elocutio.sqlite3`).
//!
//! Stores and synchronizes all personalization, encrypted API keys, and token usage locally:
//! - `personal_dictionary`: User-added vocabulary and protected terms
//! - `editorial_rules`: Rule overrides (disabled/customized rules)
//! - `learned_edit_events`: Accepted, dismissed, and edited-after-accept suggestion signals
//! - `voice_dna_traits`: Extracted stylistic traits and manual overrides
//! - `user_preferences`: Dialect, tone, custom editorial instructions, budget, and cost cap
//! - `encrypted_credentials`: AES-256 encrypted API keys (via macOS CommonCrypto)
//! - `ai_token_usage`: Token and spend ledger per AI request

use crate::config::Config;
use base64::{Engine as _, engine::general_purpose};
use harper_core::spell::Dictionary;
use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io::Read;
use std::path::PathBuf;
use std::ptr;

pub const DB_FILENAME: &str = "elocutio.sqlite3";
pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedEditSummary {
    pub kind: String,
    pub category: String,
    pub original: String,
    pub replacement: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryBreakdownItem {
    pub category: String,
    pub accepted: usize,
    pub dismissed: usize,
    pub mastery_pct: u32,
    pub insight: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsageBreakdown {
    pub model: String,
    pub requests: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub words_refined: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyActivityPoint {
    pub day_label: String,
    pub ai_passes: usize,
    pub inline_edits: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingAnalyticsSnapshot {
    pub prose_craft_score: u32,
    pub acceptance_rate_pct: u32,
    pub total_accepted_edits: usize,
    pub total_dismissed_edits: usize,
    pub total_ai_requests: usize,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_ai_words_refined: u64,
    pub total_cost_usd: f64,
    pub avg_cost_per_pass_usd: f64,
    pub estimated_minutes_saved: u32,
    pub vocabulary_shield_words: usize,
    pub active_rules_count: usize,
    pub voice_traits_calibrated: usize,
    pub active_days_14d: usize,
    pub local_vs_ai_pct: u32,
    pub category_breakdown: Vec<CategoryBreakdownItem>,
    pub model_breakdown: Vec<ModelUsageBreakdown>,
    pub daily_activity: Vec<DailyActivityPoint>,
    pub top_transformations: Vec<LearnedEditSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalizationDbStatus {
    pub db_path: String,
    pub db_filename: String,
    pub db_size_bytes: u64,
    pub schema_version: u32,
    pub dictionary_word_count: usize,
    pub builtin_word_count: usize,
    pub customized_rule_count: usize,
    pub total_rule_count: usize,
    pub learned_events_count: usize,
    pub voice_traits_count: usize,
    pub learn_from_edits: bool,
    pub recent_learned_edits: Vec<LearnedEditSummary>,
}

#[link(name = "sqlite3")]
unsafe extern "C" {
    fn sqlite3_open(filename: *const c_char, pp_db: *mut *mut c_void) -> c_int;
    fn sqlite3_close(db: *mut c_void) -> c_int;
    fn sqlite3_exec(
        db: *mut c_void,
        sql: *const c_char,
        callback: Option<
            unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int,
        >,
        arg: *mut c_void,
        errmsg: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_free(ptr: *mut c_void);
}

// macOS CommonCrypto (`libSystem.B.dylib`) AES-256-CBC + PBKDF2-HMAC-SHA256
unsafe extern "C" {
    fn CCKeyDerivationPBKDF(
        algorithm: u32,
        password: *const c_char,
        password_len: usize,
        salt: *const u8,
        salt_len: usize,
        prf: u32,
        rounds: u32,
        derived_key: *mut u8,
        derived_key_len: usize,
    ) -> i32;

    fn CCCrypt(
        op: u32,
        alg: u32,
        options: u32,
        key: *const c_void,
        key_length: usize,
        iv: *const c_void,
        data_in: *const c_void,
        data_in_length: usize,
        data_out: *mut c_void,
        data_out_available: usize,
        data_out_moved: *mut usize,
    ) -> i32;
}

const K_CC_PBKDF2: u32 = 2;
const K_CC_PRF_HMAC_ALG_SHA256: u32 = 3;
const K_CC_ENCRYPT: u32 = 0;
const K_CC_DECRYPT: u32 = 1;
const K_CC_ALGORITHM_AES128: u32 = 0; // AES block cipher (supports 128/192/256-bit keys)
const K_CC_OPTION_PKCS7_PADDING: u32 = 1;
const AES_256_KEY_LEN: usize = 32;
const AES_BLOCK_IV_LEN: usize = 16;

fn derive_vault_key_256() -> Result<[u8; AES_256_KEY_LEN], String> {
    let user = std::env::var("USER").unwrap_or_else(|_| "macos-user".to_string());
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/default".to_string());
    let passphrase = format!("elocutio.local.vault.v2::{user}::{home}");
    let salt = b"app.elocutio.desktop.sqlite3.aes256.salt";
    let mut derived = [0u8; AES_256_KEY_LEN];
    let status = unsafe {
        CCKeyDerivationPBKDF(
            K_CC_PBKDF2,
            passphrase.as_ptr() as *const c_char,
            passphrase.len(),
            salt.as_ptr(),
            salt.len(),
            K_CC_PRF_HMAC_ALG_SHA256,
            10_000,
            derived.as_mut_ptr(),
            derived.len(),
        )
    };
    if status != 0 {
        return Err(format!("PBKDF2 key derivation failed ({status})"));
    }
    Ok(derived)
}

fn random_iv_16() -> [u8; AES_BLOCK_IV_LEN] {
    let mut iv = [0u8; AES_BLOCK_IV_LEN];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut iv);
        if iv.iter().any(|&b| b != 0) {
            return iv;
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0x9e3779b97f4a7c15);
    iv.copy_from_slice(&nanos.to_le_bytes());
    iv
}

fn encrypt_aes256_cbc(plaintext: &str) -> Result<(String, String), String> {
    let key = derive_vault_key_256()?;
    let iv = random_iv_16();
    let input = plaintext.as_bytes();
    let mut out = vec![0u8; input.len() + AES_BLOCK_IV_LEN * 2];
    let mut moved: usize = 0;

    let status = unsafe {
        CCCrypt(
            K_CC_ENCRYPT,
            K_CC_ALGORITHM_AES128,
            K_CC_OPTION_PKCS7_PADDING,
            key.as_ptr() as *const c_void,
            key.len(),
            iv.as_ptr() as *const c_void,
            input.as_ptr() as *const c_void,
            input.len(),
            out.as_mut_ptr() as *mut c_void,
            out.len(),
            &mut moved,
        )
    };
    if status != 0 {
        return Err(format!("AES-256 encryption failed ({status})"));
    }
    out.truncate(moved);
    Ok((
        general_purpose::STANDARD.encode(&out),
        general_purpose::STANDARD.encode(iv),
    ))
}

fn decrypt_aes256_cbc(ciphertext_b64: &str, iv_b64: &str) -> Result<String, String> {
    let key = derive_vault_key_256()?;
    let ciphertext = general_purpose::STANDARD
        .decode(ciphertext_b64.trim())
        .map_err(|e| format!("Invalid ciphertext base64: {e}"))?;
    let iv = general_purpose::STANDARD
        .decode(iv_b64.trim())
        .map_err(|e| format!("Invalid IV base64: {e}"))?;
    if iv.len() != AES_BLOCK_IV_LEN {
        return Err("Invalid AES IV length".to_string());
    }

    let mut out = vec![0u8; ciphertext.len() + AES_BLOCK_IV_LEN];
    let mut moved: usize = 0;
    let status = unsafe {
        CCCrypt(
            K_CC_DECRYPT,
            K_CC_ALGORITHM_AES128,
            K_CC_OPTION_PKCS7_PADDING,
            key.as_ptr() as *const c_void,
            key.len(),
            iv.as_ptr() as *const c_void,
            ciphertext.as_ptr() as *const c_void,
            ciphertext.len(),
            out.as_mut_ptr() as *mut c_void,
            out.len(),
            &mut moved,
        )
    };
    if status != 0 {
        return Err(format!("AES-256 decryption failed ({status})"));
    }
    out.truncate(moved);
    String::from_utf8(out).map_err(|e| format!("Decrypted key is not valid UTF-8: {e}"))
}

pub fn mask_key_hint(raw_key: &str) -> String {
    let trimmed = raw_key.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 8 {
        "••••".to_string()
    } else {
        let prefix: String = chars[..6].iter().collect();
        let suffix: String = chars[chars.len() - 4..].iter().collect();
        format!("{prefix}…{suffix}")
    }
}

struct SqliteConn(*mut c_void);

impl SqliteConn {
    fn open(path: &std::path::Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create database folder: {e}"))?;
        }
        let path_str = path.to_string_lossy();
        let c_path =
            CString::new(path_str.as_bytes()).map_err(|e| format!("Invalid database path: {e}"))?;
        let mut db: *mut c_void = ptr::null_mut();
        let rc = unsafe { sqlite3_open(c_path.as_ptr(), &mut db) };
        if rc != 0 || db.is_null() {
            if !db.is_null() {
                unsafe {
                    sqlite3_close(db);
                }
            }
            return Err(format!("sqlite3_open failed with code {rc}"));
        }
        let conn = Self(db);
        conn.exec_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS schema_meta (
                 version INTEGER PRIMARY KEY,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS personal_dictionary (
                 word TEXT PRIMARY KEY,
                 source TEXT NOT NULL DEFAULT 'user',
                 added_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS editorial_rules (
                 rule_key TEXT PRIMARY KEY,
                 enabled INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS learned_edit_events (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 kind TEXT NOT NULL,
                 category TEXT NOT NULL,
                 original TEXT NOT NULL,
                 replacement TEXT NOT NULL,
                 origin TEXT NOT NULL,
                 timestamp INTEGER NOT NULL,
                 UNIQUE(kind, category, original, replacement, timestamp)
             );
             CREATE TABLE IF NOT EXISTS voice_dna_traits (
                 trait_key TEXT PRIMARY KEY,
                 label TEXT NOT NULL,
                 summary TEXT NOT NULL,
                 confidence REAL NOT NULL,
                 is_override INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS user_preferences (
                 pref_key TEXT PRIMARY KEY,
                 pref_value TEXT NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS encrypted_credentials (
                 provider_id TEXT PRIMARY KEY,
                 ciphertext_b64 TEXT NOT NULL,
                 iv_b64 TEXT NOT NULL,
                 key_hint TEXT NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS ai_token_usage (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 timestamp INTEGER NOT NULL,
                 model TEXT NOT NULL,
                 input_tokens INTEGER NOT NULL,
                 output_tokens INTEGER NOT NULL,
                 cost_usd REAL,
                 UNIQUE(timestamp, model, input_tokens, output_tokens)
             );",
        )?;
        Ok(conn)
    }

    fn exec_batch(&self, sql: &str) -> Result<(), String> {
        let c_sql = CString::new(sql).map_err(|e| format!("Invalid SQL string: {e}"))?;
        let mut errmsg: *mut c_char = ptr::null_mut();
        let rc =
            unsafe { sqlite3_exec(self.0, c_sql.as_ptr(), None, ptr::null_mut(), &mut errmsg) };
        if rc != 0 {
            let msg = if !errmsg.is_null() {
                let s = unsafe { CStr::from_ptr(errmsg).to_string_lossy().into_owned() };
                unsafe { sqlite3_free(errmsg as *mut c_void) };
                s
            } else {
                format!("SQLite error code {rc}")
            };
            return Err(msg);
        }
        Ok(())
    }

    fn query_rows(&self, sql: &str) -> Result<Vec<Vec<String>>, String> {
        unsafe extern "C" fn collect_rows_cb(
            arg: *mut c_void,
            argc: c_int,
            argv: *mut *mut c_char,
            _col_names: *mut *mut c_char,
        ) -> c_int {
            if argc > 0 && !argv.is_null() {
                let mut row = Vec::with_capacity(argc as usize);
                for i in 0..(argc as usize) {
                    let val_ptr = unsafe { *argv.add(i) };
                    if !val_ptr.is_null() {
                        let val = unsafe { CStr::from_ptr(val_ptr).to_string_lossy().into_owned() };
                        row.push(val);
                    } else {
                        row.push(String::new());
                    }
                }
                let out = unsafe { &mut *(arg as *mut Vec<Vec<String>>) };
                out.push(row);
            }
            0
        }

        let mut results: Vec<Vec<String>> = Vec::new();
        let c_sql = CString::new(sql).map_err(|e| format!("Invalid SQL string: {e}"))?;
        let mut errmsg: *mut c_char = ptr::null_mut();
        let rc = unsafe {
            sqlite3_exec(
                self.0,
                c_sql.as_ptr(),
                Some(collect_rows_cb),
                (&mut results as *mut Vec<Vec<String>>) as *mut c_void,
                &mut errmsg,
            )
        };
        if rc != 0 {
            if !errmsg.is_null() {
                unsafe { sqlite3_free(errmsg as *mut c_void) };
            }
            return Err(format!("SQLite query failed with code {rc}"));
        }
        Ok(results)
    }

    fn query_single_column(&self, sql: &str) -> Result<Vec<String>, String> {
        Ok(self
            .query_rows(sql)?
            .into_iter()
            .filter_map(|mut r| {
                if r.is_empty() {
                    None
                } else {
                    Some(r.swap_remove(0))
                }
            })
            .collect())
    }
}

impl Drop for SqliteConn {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                sqlite3_close(self.0);
            }
        }
    }
}

fn sql_escape(input: &str) -> String {
    input.replace('\'', "''")
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn db_path() -> Result<PathBuf, String> {
    #[cfg(test)]
    {
        return Ok(std::env::temp_dir().join("elocutio-test").join(DB_FILENAME));
    }
    #[cfg(not(test))]
    {
        Config::folder_path()
            .map(|folder| folder.join(DB_FILENAME))
            .ok_or_else(|| "Application support folder unavailable.".to_string())
    }
}

/// Encrypts `raw_key` with AES-256-CBC and stores it in `encrypted_credentials` in `elocutio.sqlite3`.
pub fn save_encrypted_api_key(provider_id: &str, raw_key: &str) -> Result<String, String> {
    let trimmed = raw_key.trim();
    if trimmed.is_empty() {
        return Err("Enter an API key first.".to_string());
    }
    let (ciphertext_b64, iv_b64) = encrypt_aes256_cbc(trimmed)?;
    let hint = mask_key_hint(trimmed);
    let now = now_unix();
    let path = db_path()?;
    let conn = SqliteConn::open(&path)?;
    let sql = format!(
        "INSERT OR REPLACE INTO encrypted_credentials (provider_id, ciphertext_b64, iv_b64, key_hint, updated_at) VALUES ('{}', '{}', '{}', '{}', {});",
        sql_escape(provider_id),
        sql_escape(&ciphertext_b64),
        sql_escape(&iv_b64),
        sql_escape(&hint),
        now
    );
    conn.exec_batch(&sql)?;
    Ok(hint)
}

/// Loads and decrypts an API key from `encrypted_credentials` in `elocutio.sqlite3`.
pub fn load_decrypted_api_key(provider_id: &str) -> Result<Option<String>, String> {
    let path = db_path()?;
    let conn = SqliteConn::open(&path)?;
    let sql = format!(
        "SELECT ciphertext_b64, iv_b64 FROM encrypted_credentials WHERE provider_id = '{}' LIMIT 1;",
        sql_escape(provider_id)
    );
    let rows = conn.query_rows(&sql)?;
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    if first.len() < 2 {
        return Ok(None);
    }
    let decrypted = decrypt_aes256_cbc(&first[0], &first[1])?;
    if decrypted.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(decrypted))
    }
}

/// Returns the masked key hint (e.g. `sk-or-…9f2a`) if an encrypted key is stored for `provider_id`.
pub fn get_encrypted_key_hint(provider_id: &str) -> Option<String> {
    let path = db_path().ok()?;
    let conn = SqliteConn::open(&path).ok()?;
    let sql = format!(
        "SELECT key_hint FROM encrypted_credentials WHERE provider_id = '{}' LIMIT 1;",
        sql_escape(provider_id)
    );
    conn.query_single_column(&sql)
        .ok()
        .and_then(|mut v| v.pop())
        .filter(|s| !s.is_empty())
}

/// Deletes an encrypted API key for `provider_id` from `elocutio.sqlite3`.
pub fn clear_encrypted_api_key(provider_id: &str) -> Result<(), String> {
    let path = db_path()?;
    let conn = SqliteConn::open(&path)?;
    let sql = format!(
        "DELETE FROM encrypted_credentials WHERE provider_id = '{}';",
        sql_escape(provider_id)
    );
    conn.exec_batch(&sql)
}

/// Appends a token usage entry directly into `ai_token_usage` in `elocutio.sqlite3`.
pub fn record_token_usage_in_db(
    timestamp: u64,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: Option<f64>,
) {
    let Ok(path) = db_path() else {
        return;
    };
    let Ok(conn) = SqliteConn::open(&path) else {
        return;
    };
    let cost_sql = cost_usd
        .map(|c| format!("{c}"))
        .unwrap_or_else(|| "NULL".to_string());
    let sql = format!(
        "INSERT OR IGNORE INTO ai_token_usage (timestamp, model, input_tokens, output_tokens, cost_usd) VALUES ({}, '{}', {}, {}, {});",
        timestamp,
        sql_escape(model),
        input_tokens,
        output_tokens,
        cost_sql
    );
    let _ = conn.exec_batch(&sql);
}

/// Returns true only for valid single-token lexicon words (no spaces or sentences).
pub fn is_valid_dictionary_word(word: &str) -> bool {
    let trimmed = word.trim();
    !trimmed.is_empty()
        && trimmed.chars().count() <= 48
        && !trimmed.chars().any(char::is_whitespace)
}

/// Reads any words stored in `personal_dictionary` in `elocutio.sqlite3`.
pub fn load_dictionary_words_from_db() -> Vec<String> {
    let Ok(path) = db_path() else {
        return Vec::new();
    };
    let Ok(conn) = SqliteConn::open(&path) else {
        return Vec::new();
    };
    conn.query_single_column("SELECT word FROM personal_dictionary ORDER BY word ASC;")
        .unwrap_or_default()
        .into_iter()
        .filter(|w| is_valid_dictionary_word(w))
        .collect()
}

/// Synchronizes all in-memory and file-based personalization into `elocutio.sqlite3`
/// and returns a live status snapshot for the UI.
pub fn sync_and_status(config: &Config) -> Result<PersonalizationDbStatus, String> {
    let path = db_path()?;
    let conn = SqliteConn::open(&path)?;
    let now = now_unix();

    let mut batch = String::from("BEGIN TRANSACTION;\n");
    batch.push_str(&format!(
        "INSERT OR REPLACE INTO schema_meta (version, updated_at) VALUES ({SCHEMA_VERSION}, {now});\n"
    ));

    // 1. Sync personal dictionary words (single-token words only)
    batch.push_str("DELETE FROM personal_dictionary;\n");
    let mut dict_words: Vec<String> = config
        .mutable_dictionary
        .words_iter()
        .map(|w| w.iter().collect::<String>())
        .filter(|w| is_valid_dictionary_word(w))
        .collect();
    dict_words.sort();
    dict_words.dedup();
    for word in &dict_words {
        let escaped = sql_escape(word);
        batch.push_str(&format!(
            "INSERT OR REPLACE INTO personal_dictionary (word, source, added_at) VALUES ('{escaped}', 'user', {now});\n"
        ));
    }

    // 2. Sync editorial rules and count customized rules
    let mut total_rule_count = 0usize;
    let mut customized_rule_count = 0usize;
    batch.push_str("DELETE FROM editorial_rules;\n");
    if let Ok(serialized) = serde_json::to_value(&config.lint_config)
        && let Some(map) = serialized.as_object()
    {
        total_rule_count = map.len();
        for (key, val) in map {
            let enabled = val.as_bool().unwrap_or(true);
            if !enabled {
                customized_rule_count += 1;
                let escaped_key = sql_escape(key);
                batch.push_str(&format!(
                    "INSERT OR REPLACE INTO editorial_rules (rule_key, enabled, updated_at) VALUES ('{escaped_key}', 0, {now});\n"
                ));
            }
        }
    }

    // 3. Sync learned edit feedback events from voice-feedback.jsonl
    let mut recent_learned_edits: Vec<LearnedEditSummary> = Vec::new();
    let mut learned_events_count = 0usize;
    if let Some(folder) = Config::folder_path() {
        let feedback_file = folder.join("voice-feedback.jsonl");
        if let Ok(contents) = std::fs::read_to_string(feedback_file) {
            for line in contents.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                    let kind = val
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or("accepted")
                        .to_string();
                    let category = val
                        .get("category")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Grammar")
                        .to_string();
                    let original = val
                        .get("original")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let replacement = val
                        .get("replacement")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let origin = val
                        .get("origin")
                        .and_then(|v| v.as_str())
                        .unwrap_or("user")
                        .to_string();
                    let timestamp = val.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(now);

                    learned_events_count += 1;
                    batch.push_str(&format!(
                        "INSERT OR IGNORE INTO learned_edit_events (kind, category, original, replacement, origin, timestamp) VALUES ('{}', '{}', '{}', '{}', '{}', {});\n",
                        sql_escape(&kind),
                        sql_escape(&category),
                        sql_escape(&original),
                        sql_escape(&replacement),
                        sql_escape(&origin),
                        timestamp
                    ));
                    recent_learned_edits.push(LearnedEditSummary {
                        kind,
                        category,
                        original,
                        replacement,
                        timestamp,
                    });
                }
            }
        }
    }
    recent_learned_edits.reverse();
    recent_learned_edits.truncate(12);

    // 4. Sync Voice DNA traits
    let traits = crate::voice::traits_for_ui();
    let voice_traits_count = traits.len();
    batch.push_str("DELETE FROM voice_dna_traits;\n");
    for t in &traits {
        let val_str = serde_json::to_string(&t.value).unwrap_or_default();
        let summary_str = t
            .evidence
            .first()
            .cloned()
            .unwrap_or_else(|| val_str.clone());
        batch.push_str(&format!(
            "INSERT OR REPLACE INTO voice_dna_traits (trait_key, label, summary, confidence, is_override, updated_at) VALUES ('{}', '{}', '{}', {}, {}, {});\n",
            sql_escape(&t.key),
            sql_escape(&val_str),
            sql_escape(&summary_str),
            t.confidence,
            if t.is_override { 1 } else { 0 },
            now
        ));
    }

    // 5. Sync core user preferences (including budget_usd & cost_cap_usd)
    let dialect_str = format!("{:?}", config.dialect);
    let tone_str = format!("{:?}", config.rewrite_tone);
    let budget_str = config
        .budget_usd
        .map(|b| format!("{b:.2}"))
        .unwrap_or_default();
    let cost_cap_str = config
        .cost_cap_usd
        .map(|c| format!("{c:.4}"))
        .unwrap_or_default();
    for (k, v) in [
        ("dialect", dialect_str.as_str()),
        (
            "learn_from_edits",
            if config.learn_from_edits || crate::voice::is_learning_enabled() {
                "true"
            } else {
                "false"
            },
        ),
        ("rewrite_tone", tone_str.as_str()),
        ("custom_instruction", config.custom_instruction.as_str()),
        ("provider_base_url", config.provider_base_url.as_str()),
        ("provider_model", config.provider_model.as_str()),
        ("budget_usd", budget_str.as_str()),
        ("cost_cap_usd", cost_cap_str.as_str()),
    ] {
        batch.push_str(&format!(
            "INSERT OR REPLACE INTO user_preferences (pref_key, pref_value, updated_at) VALUES ('{}', '{}', {});\n",
            sql_escape(k),
            sql_escape(v),
            now
        ));
    }

    // 6. Sync usage.jsonl into ai_token_usage table
    if let Some(folder) = Config::folder_path() {
        let usage_file = folder.join("usage.jsonl");
        if let Ok(contents) = std::fs::read_to_string(usage_file) {
            for line in contents.lines().rev().take(200) {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(entry) = serde_json::from_str::<crate::usage::UsageEntry>(line) {
                    let cost_sql = entry
                        .cost_usd
                        .map(|c| format!("{c}"))
                        .unwrap_or_else(|| "NULL".to_string());
                    batch.push_str(&format!(
                        "INSERT OR IGNORE INTO ai_token_usage (timestamp, model, input_tokens, output_tokens, cost_usd) VALUES ({}, '{}', {}, {}, {});\n",
                        entry.timestamp,
                        sql_escape(&entry.model),
                        entry.input_tokens,
                        entry.output_tokens,
                        cost_sql
                    ));
                }
            }
        }
    }

    batch.push_str("COMMIT;\n");
    conn.exec_batch(&batch)?;
    drop(conn);

    // 7. Automatically migrate any legacy plaintext keys in credentials.json into encrypted_credentials
    if let Some(folder) = Config::folder_path() {
        let cred_file = folder.join("credentials.json");
        if let Ok(contents) = std::fs::read_to_string(&cred_file)
            && let Ok(map) =
                serde_json::from_str::<std::collections::BTreeMap<String, String>>(&contents)
        {
            for (k, v) in &map {
                let trimmed = v.trim();
                if !trimmed.is_empty() && get_encrypted_key_hint(k).is_none() {
                    let _ = save_encrypted_api_key(k, trimmed);
                }
            }
        }
    }

    let db_size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

    Ok(PersonalizationDbStatus {
        db_path: path.to_string_lossy().into_owned(),
        db_filename: DB_FILENAME.to_string(),
        db_size_bytes,
        schema_version: SCHEMA_VERSION,
        dictionary_word_count: dict_words.len(),
        builtin_word_count: 29,
        customized_rule_count,
        total_rule_count,
        learned_events_count,
        voice_traits_count,
        learn_from_edits: config.learn_from_edits || crate::voice::is_learning_enabled(),
        recent_learned_edits,
    })
}

/// Computes comprehensive writing & AI co-pilot analytics from `elocutio.sqlite3`.
pub fn get_writing_analytics(config: &Config) -> Result<WritingAnalyticsSnapshot, String> {
    let db_status = sync_and_status(config)?;
    let path = db_path()?;
    let conn = SqliteConn::open(&path)?;

    // 1. Query learned_edit_events
    let event_rows = conn
        .query_rows(
            "SELECT kind, category, original, replacement, timestamp FROM learned_edit_events ORDER BY timestamp DESC;",
        )
        .unwrap_or_default();

    let mut total_accepted_edits = 0usize;
    let mut total_dismissed_edits = 0usize;
    let mut cat_counts: [(usize, usize); 4] = [(0, 0); 4]; // (accepted, dismissed) for 4 pillars
    let mut top_transformations: Vec<LearnedEditSummary> = Vec::new();
    let mut seen_pairs = std::collections::HashSet::new();
    let mut edit_day_counts = std::collections::HashMap::<u64, usize>::new();
    let mut active_day_set = std::collections::HashSet::<u64>::new();
    let now = now_unix();
    let today_bucket = now / 86_400;

    for row in &event_rows {
        let kind = row.first().map(|s| s.as_str()).unwrap_or("accepted");
        let category = row.get(1).map(|s| s.as_str()).unwrap_or("Grammar");
        let original = row.get(2).cloned().unwrap_or_default();
        let replacement = row.get(3).cloned().unwrap_or_default();
        let timestamp = row
            .get(4)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(now);

        let is_accepted = kind == "accepted";
        if is_accepted {
            total_accepted_edits += 1;
        } else {
            total_dismissed_edits += 1;
        }

        let day_bucket = timestamp / 86_400;
        *edit_day_counts.entry(day_bucket).or_insert(0) += 1;
        if today_bucket.saturating_sub(day_bucket) < 14 {
            active_day_set.insert(day_bucket);
        }

        let cat_lower = category.to_lowercase();
        let pillar_idx = if cat_lower.contains("clarity")
            || cat_lower.contains("wordi")
            || cat_lower.contains("concise")
            || cat_lower.contains("readab")
            || cat_lower.contains("passive")
        {
            1
        } else if cat_lower.contains("style")
            || cat_lower.contains("redund")
            || cat_lower.contains("repet")
            || cat_lower.contains("enhance")
            || cat_lower.contains("choice")
        {
            2
        } else if cat_lower.contains("tone")
            || cat_lower.contains("deliver")
            || cat_lower.contains("confiden")
            || cat_lower.contains("formal")
            || cat_lower.contains("rewrite")
        {
            3
        } else {
            0
        };

        if is_accepted {
            cat_counts[pillar_idx].0 += 1;
        } else {
            cat_counts[pillar_idx].1 += 1;
        }

        if is_accepted
            && !original.trim().is_empty()
            && !replacement.trim().is_empty()
            && original != replacement
            && top_transformations.len() < 8
        {
            let pair_key = format!("{}->{}", original.trim(), replacement.trim());
            if seen_pairs.insert(pair_key) {
                top_transformations.push(LearnedEditSummary {
                    kind: kind.to_string(),
                    category: category.to_string(),
                    original,
                    replacement,
                    timestamp,
                });
            }
        }
    }

    // 2. Query ai_token_usage grouped by model
    let model_rows = conn
        .query_rows(
            "SELECT model, COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0), COALESCE(SUM(cost_usd), 0.0) FROM ai_token_usage GROUP BY model ORDER BY COUNT(*) DESC;",
        )
        .unwrap_or_default();

    let mut total_ai_requests = 0usize;
    let mut total_input_tokens = 0u64;
    let mut total_output_tokens = 0u64;
    let mut total_cost_usd = 0.0f64;
    let mut model_breakdown: Vec<ModelUsageBreakdown> = Vec::new();

    for row in &model_rows {
        let model = row
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        let requests = row
            .get(1)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        let input_tokens = row.get(2).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        let output_tokens = row.get(3).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        let cost_usd = row
            .get(4)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let words_refined = ((output_tokens as f64) * 0.75).round() as u64;

        total_ai_requests += requests;
        total_input_tokens += input_tokens;
        total_output_tokens += output_tokens;
        total_cost_usd += cost_usd;

        model_breakdown.push(ModelUsageBreakdown {
            model,
            requests,
            input_tokens,
            output_tokens,
            words_refined,
            cost_usd,
        });
    }

    // 3. Query daily AI activity timestamps
    let ai_ts_rows = conn
        .query_single_column("SELECT timestamp FROM ai_token_usage;")
        .unwrap_or_default();
    let mut ai_day_counts = std::collections::HashMap::<u64, usize>::new();
    for ts_str in ai_ts_rows {
        if let Ok(ts) = ts_str.parse::<u64>() {
            let day_bucket = ts / 86_400;
            *ai_day_counts.entry(day_bucket).or_insert(0) += 1;
            if today_bucket.saturating_sub(day_bucket) < 14 {
                active_day_set.insert(day_bucket);
            }
        }
    }

    let weekday_names = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let mut daily_activity = Vec::with_capacity(7);
    for offset in (0..7).rev() {
        let bucket = today_bucket.saturating_sub(offset);
        let label = if offset == 0 {
            "Today".to_string()
        } else if offset == 1 {
            "Yesterday".to_string()
        } else {
            weekday_names[(bucket as usize) % 7].to_string()
        };
        daily_activity.push(DailyActivityPoint {
            day_label: label,
            ai_passes: *ai_day_counts.get(&bucket).unwrap_or(&0),
            inline_edits: *edit_day_counts.get(&bucket).unwrap_or(&0),
        });
    }

    let total_ai_words_refined = ((total_output_tokens as f64) * 0.75).round() as u64;
    let avg_cost_per_pass_usd = if total_ai_requests > 0 {
        total_cost_usd / (total_ai_requests as f64)
    } else {
        0.0
    };

    let total_decisions = total_accepted_edits + total_dismissed_edits;
    let acceptance_rate_pct = if total_decisions > 0 {
        ((total_accepted_edits * 100) / total_decisions) as u32
    } else {
        88
    };

    let total_seconds_saved = (total_accepted_edits as u32 * 14) + (total_ai_requests as u32 * 26);
    let estimated_minutes_saved = if total_seconds_saved > 0 {
        (total_seconds_saved / 60).max(1)
    } else {
        0
    };

    let vocabulary_shield_words = db_status.dictionary_word_count + db_status.builtin_word_count;
    let active_rules_count = db_status
        .total_rule_count
        .saturating_sub(db_status.customized_rule_count);

    let prose_craft_score = (74
        + (acceptance_rate_pct / 6).min(15)
        + (db_status.voice_traits_count.min(4) as u32 * 2)
        + if total_ai_requests > 0 { 3 } else { 0 })
    .clamp(68, 99);

    let local_weight = (total_decisions + vocabulary_shield_words * 2).max(1);
    let total_hybrid_weight = local_weight + total_ai_requests;
    let local_vs_ai_pct = ((local_weight * 100) / total_hybrid_weight).clamp(25, 95) as u32;

    let pillar_defs = [
        (
            "Grammar & Mechanics",
            cat_counts[0].0,
            cat_counts[0].1,
            94u32,
            "High precision on punctuation & agreement",
        ),
        (
            "Clarity & Conciseness",
            cat_counts[1].0,
            cat_counts[1].1,
            89u32,
            "Tightens wordy clauses & passive phrasing",
        ),
        (
            "Style & Elegance",
            cat_counts[2].0,
            cat_counts[2].1,
            91u32,
            "Eliminates repetition & weak modifiers",
        ),
        (
            "Tone & AI Rewrites",
            cat_counts[3].0 + (total_ai_requests / 4),
            cat_counts[3].1,
            93u32,
            "Deep structural & tonal refinement via AI",
        ),
    ];

    let category_breakdown = pillar_defs
        .into_iter()
        .map(|(name, acc, dis, base_mastery, insight)| {
            let total = acc + dis;
            let mastery_pct = if total > 0 {
                ((base_mastery + ((acc * 100 / total) as u32)) / 2).clamp(62, 99)
            } else {
                base_mastery
            };
            CategoryBreakdownItem {
                category: name.to_string(),
                accepted: acc,
                dismissed: dis,
                mastery_pct,
                insight: insight.to_string(),
            }
        })
        .collect();

    Ok(WritingAnalyticsSnapshot {
        prose_craft_score,
        acceptance_rate_pct,
        total_accepted_edits,
        total_dismissed_edits,
        total_ai_requests,
        total_input_tokens,
        total_output_tokens,
        total_ai_words_refined,
        total_cost_usd,
        avg_cost_per_pass_usd,
        estimated_minutes_saved,
        vocabulary_shield_words,
        active_rules_count,
        voice_traits_calibrated: db_status.voice_traits_count,
        active_days_14d: active_day_set.len().max(1),
        local_vs_ai_pct,
        category_breakdown,
        model_breakdown,
        daily_activity,
        top_transformations,
    })
}
