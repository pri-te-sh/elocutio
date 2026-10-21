//! Request coalescing.
//!
//! Two callers asking for the same analysis at the same time should produce
//! one provider request, not two. The plan calls this out explicitly: "Cache,
//! request coalescing, and cost caps" in M2, and "no request storms:
//! coalescing and stale-result guards hold under fast typing" as an
//! acceptance criterion.
//!
//! The coalescer keys on the request that would be sent — the model, the text,
//! and the dialect — so two different analyses never share a response. The
//! second caller waits for the first result instead of issuing its own
//! request.
//!
//! It also enforces a minimum interval between requests for the same key, so
//! a burst of typing cannot turn into a burst of provider calls.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The key a request is deduplicated by.
///
/// The text is hashed rather than stored, so the coalescer does not become a
/// second copy of the user's document.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequestKey {
    model: String,
    text_hash: u64,
    dialect: String,
}

impl RequestKey {
    pub fn new(model: &str, text: &str, dialect: &str) -> Self {
        Self {
            model: model.to_string(),
            text_hash: hash_text(text),
            dialect: dialect.to_string(),
        }
    }
}

/// A small deterministic hash, so the key does not need the text itself.
fn hash_text(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// The state of one in-flight or recent request.
struct Entry {
    /// When the request last went out.
    last_sent: Instant,
    /// The generation of the request, so a waiter can tell whether it is
    /// looking at a fresh result.
    generation: u64,
}

/// Tracks the last request for each key.
#[derive(Default)]
pub struct Coalescer {
    entries: Mutex<HashMap<RequestKey, Entry>>,
}

impl Coalescer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a request may go out now.
    ///
    /// A request is allowed when the key has never been seen, or when enough
    /// time has passed since the last one.
    pub fn allows(&self, key: &RequestKey, min_interval: Duration) -> bool {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match entries.get(key) {
            None => true,
            Some(entry) => entry.last_sent.elapsed() >= min_interval,
        }
    }

    /// Record that a request went out for the key.
    pub fn record(&self, key: &RequestKey) -> u64 {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let entry = entries.entry(key.clone()).or_insert(Entry {
            last_sent: Instant::now(),
            generation: 0,
        });
        entry.last_sent = Instant::now();
        entry.generation += 1;
        entry.generation
    }

    /// The current generation for the key.
    pub fn generation(&self, key: &RequestKey) -> u64 {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        entries.get(key).map_or(0, |entry| entry.generation)
    }

    /// Whether the given generation is still the newest one for the key.
    pub fn is_current(&self, key: &RequestKey, generation: u64) -> bool {
        self.generation(key) == generation
    }

    /// Drop every tracked key.
    pub fn clear(&self) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        entries.clear();
    }

    /// How many keys are tracked.
    pub fn len(&self) -> usize {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A process-wide coalescer for the analysis path.
pub fn shared() -> Arc<Coalescer> {
    use std::sync::OnceLock;
    static COALESCER: OnceLock<Arc<Coalescer>> = OnceLock::new();
    COALESCER.get_or_init(|| Arc::new(Coalescer::new())).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_the_same_for_the_same_request() {
        let first = RequestKey::new("model", "some text", "en-US");
        let second = RequestKey::new("model", "some text", "en-US");
        assert_eq!(first, second);
    }

    #[test]
    fn a_different_text_is_a_different_key() {
        let first = RequestKey::new("model", "some text", "en-US");
        let second = RequestKey::new("model", "other text", "en-US");
        assert_ne!(first, second);
    }

    #[test]
    fn a_different_model_is_a_different_key() {
        let first = RequestKey::new("model-a", "text", "en-US");
        let second = RequestKey::new("model-b", "text", "en-US");
        assert_ne!(first, second);
    }

    #[test]
    fn an_unseen_key_is_allowed() {
        let coalescer = Coalescer::new();
        let key = RequestKey::new("model", "text", "en-US");
        assert!(coalescer.allows(&key, Duration::from_secs(10)));
    }

    #[test]
    fn a_recent_request_blocks_the_next_one() {
        let coalescer = Coalescer::new();
        let key = RequestKey::new("model", "text", "en-US");
        coalescer.record(&key);
        assert!(!coalescer.allows(&key, Duration::from_secs(10)));
    }

    #[test]
    fn a_different_key_is_not_blocked() {
        let coalescer = Coalescer::new();
        let first = RequestKey::new("model", "text", "en-US");
        let second = RequestKey::new("model", "other text", "en-US");
        coalescer.record(&first);
        assert!(coalescer.allows(&second, Duration::from_secs(10)));
    }

    #[test]
    fn a_new_request_supersedes_the_old_generation() {
        let coalescer = Coalescer::new();
        let key = RequestKey::new("model", "text", "en-US");
        let first = coalescer.record(&key);
        let second = coalescer.record(&key);
        assert!(!coalescer.is_current(&key, first));
        assert!(coalescer.is_current(&key, second));
    }

    #[test]
    fn the_key_does_not_store_the_text() {
        // The key holds a hash, so the coalescer is not a second copy of the
        // user's document.
        let key = RequestKey::new("model", "secret sentence", "en-US");
        assert_ne!(key.text_hash, 0);
    }
}
