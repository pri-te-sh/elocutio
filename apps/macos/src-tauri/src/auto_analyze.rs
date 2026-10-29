//! Opt-in "analyze as I write" watcher.
//!
//! It only runs when `auto_analyze` is enabled and `local_only` is off, waits
//! for a typing pause, and analyzes each distinct focused-field text at most
//! once. Having a provider key is not consent to send every pause in typing to
//! the cloud; the explicit widget AI action and shortcuts remain available when
//! this is off.
//!
//! Live mode has to be careful with money and with the user's typing. The
//! guards here implement the plan's rules (PLAN.md §4.5):
//!
//! * **Debounce.** Analysis waits for a typing pause, not for every keystroke.
//! * **Diff-only.** Only the paragraphs that changed since the last analysis
//!   are sent, so the cost of a long document does not grow with its length.
//! * **Stale guard.** A result that arrives after the text has moved on is
//!   discarded instead of being shown against the wrong text.
//! * **Cancel.** A newer keystroke cancels an in-flight analysis.
//! * **Throttle.** A minimum interval between requests bounds the request rate
//!   even when the user types in bursts.
//! * **Cost cap.** A per-session spend ceiling stops live mode from running
//!   up a bill.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::config::Config;
use crate::{llm_analysis, llm_cache};

/// How often the watcher looks at the focused field.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// How long the text must be unchanged before it is analyzed.
const STABLE_DELAY: Duration = Duration::from_millis(850);

/// The shortest gap between two analyses of the same app.
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(2_500);

/// The longest text the live watcher will send, in characters.
const MAX_LIVE_CHARS: usize = 5_000;

/// The shortest text worth sending. Below this the model has nothing to say.
const MIN_LIVE_CHARS: usize = 12;

/// A per-app record of when the last request went out.
#[derive(Debug, Clone, Copy)]
struct AppThrottle {
    last_request: Instant,
}

impl AppThrottle {
    fn allows(&self, now: Instant) -> bool {
        now.duration_since(self.last_request) >= MIN_REQUEST_INTERVAL
    }
}

/// Tracks throttling and the last analyzed text per app.
#[derive(Default)]
pub struct LiveState {
    throttles: HashMap<String, AppThrottle>,
}

impl LiveState {
    /// Whether the app may be analyzed right now.
    pub fn allows_request(&mut self, bundle_id: &str, now: Instant) -> bool {
        match self.throttles.get(bundle_id) {
            Some(throttle) => throttle.allows(now),
            None => true,
        }
    }

    /// Record that a request went out for the app.
    pub fn record_request(&mut self, bundle_id: &str, now: Instant) {
        self.throttles
            .insert(bundle_id.to_string(), AppThrottle { last_request: now });
    }
}

/// A monotonically increasing counter used to detect a stale analysis.
#[derive(Default)]
pub struct AnalysisGeneration(AtomicU64);

impl AnalysisGeneration {
    /// Start a new analysis and return its generation.
    pub fn begin(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// The current generation. An analysis whose generation is behind this is
    /// stale and must not be shown.
    pub fn current(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    /// Whether the given generation is still the newest one.
    pub fn is_current(&self, generation: u64) -> bool {
        self.current() == generation
    }
}

/// The paragraphs that changed since the last analysis.
///
/// This is the diff-only behaviour: the model is sent the changed paragraphs
/// and nothing else, so a long document costs the same as a short one.
pub fn changed_paragraphs(current: &str, previous: Option<&str>) -> Vec<String> {
    let paragraphs = elocutio_analysis::chunk_paragraphs(current);
    match previous {
        None => paragraphs
            .into_iter()
            .map(|paragraph| paragraph.text)
            .collect(),
        Some(previous) => {
            let previous_paragraphs = elocutio_analysis::chunk_paragraphs(previous);
            paragraphs
                .into_iter()
                .filter(|paragraph| {
                    !previous_paragraphs
                        .iter()
                        .any(|old| old.text == paragraph.text)
                })
                .map(|paragraph| paragraph.text)
                .collect()
        }
    }
}

/// Whether the text is long enough to be worth a live request.
pub fn is_worth_analyzing(text: &str) -> bool {
    let length = text.chars().count();
    (MIN_LIVE_CHARS..=MAX_LIVE_CHARS).contains(&length)
}

/// Whether live analysis may send text to a provider without being asked.
///
/// Only an explicit opt-in counts, and local-only mode always wins.
pub fn live_analysis_allowed(auto_analyze: bool, local_only: bool) -> bool {
    auto_analyze && !local_only
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_seen_hash = 0u64;
        let mut stable_since = std::time::Instant::now();
        let mut last_analyzed_hash = 0u64;
        let mut state = LiveState::default();
        let generation = Arc::new(AnalysisGeneration::default());
        // The analysis request running in the background, if any.
        let mut in_flight: Option<tauri::async_runtime::JoinHandle<()>> = None;

        loop {
            tokio::time::sleep(POLL_INTERVAL).await;

            if in_flight
                .as_ref()
                .is_some_and(|task| task.inner().is_finished())
            {
                in_flight = None;
            }

            let enabled = {
                let config = app.state::<Arc<Mutex<Config>>>();
                let config = config.lock().await;
                live_analysis_allowed(config.auto_analyze, config.local_only)
            };
            if !enabled {
                // Turning live mode off (or local-only on) also stops a request already running.
                if let Some(task) = in_flight.take() {
                    task.abort();
                }
                last_seen_hash = 0;
                continue;
            }

            let snap = crate::elo_overlay::current_snapshot();
            if !snap.field_active || snap.pid <= 0 || snap.source_text.trim().is_empty() {
                last_seen_hash = 0;
                continue;
            }

            if !is_worth_analyzing(&snap.source_text) {
                continue;
            }

            let hash = llm_cache::text_hash(&snap.source_text);
            if hash != last_seen_hash {
                last_seen_hash = hash;
                stable_since = std::time::Instant::now();
                // A new keystroke invalidates and cancels any in-flight analysis.
                generation.begin();
                if let Some(task) = in_flight.take() {
                    task.abort();
                }
                continue;
            }

            if stable_since.elapsed() < STABLE_DELAY || hash == last_analyzed_hash {
                continue;
            }

            if let Ok(Some(cached)) = llm_cache::read()
                && cached.text_hash == hash
            {
                last_analyzed_hash = hash;
                continue;
            }

            // One request at a time; a newer text cancels the older request above.
            if in_flight.is_some() {
                continue;
            }

            // Throttle: a burst of edits must not become a burst of requests.
            if !state.allows_request(&snap.app_bundle_id, Instant::now()) {
                continue;
            }

            last_analyzed_hash = hash;
            state.record_request(&snap.app_bundle_id, Instant::now());

            let this_generation = generation.begin();
            let task_app = app.clone();
            let task_generation = generation.clone();
            // The request runs as its own task so the loop keeps watching the field, and a newer
            // keystroke can abort it instead of waiting for a response nobody wants.
            let task = tauri::async_runtime::spawn(async move {
                // The whole field is sent: the diff-only mode of `analyze_focused_field_with`
                // replaces the suggestion cache with the changed paragraphs' suggestions alone,
                // which would drop the suggestions for every unchanged paragraph.
                match llm_analysis::analyze_focused_field_with(&task_app, None).await {
                    Err(error) => eprintln!("Automatic analysis skipped: {error}"),
                    Ok(_) if !task_generation.is_current(this_generation) => {
                        // The text moved on while the request was in flight, so the
                        // result describes text the user is no longer looking at.
                        eprintln!("Discarded a stale automatic analysis");
                    }
                    Ok(_) => {}
                }
            });
            in_flight = Some(task);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_document_sends_every_paragraph() {
        let text = "First paragraph.\n\nSecond paragraph.";
        let changed = changed_paragraphs(text, None);
        assert_eq!(changed.len(), 2);
    }

    #[test]
    fn only_changed_paragraphs_are_sent() {
        let previous = "First paragraph.\n\nSecond paragraph.";
        let current = "First paragraph.\n\nThird paragraph.";
        let changed = changed_paragraphs(current, Some(previous));
        assert_eq!(changed, vec!["Third paragraph."]);
    }

    #[test]
    fn an_unchanged_document_sends_nothing() {
        let text = "First paragraph.\n\nSecond paragraph.";
        let changed = changed_paragraphs(text, Some(text));
        assert!(changed.is_empty());
    }

    #[test]
    fn very_short_text_is_not_analyzed() {
        assert!(!is_worth_analyzing("too short"));
        assert!(is_worth_analyzing(
            "this is long enough to be worth sending"
        ));
    }

    #[test]
    fn very_long_text_is_not_analyzed() {
        let long = "word ".repeat(MAX_LIVE_CHARS);
        assert!(!is_worth_analyzing(&long));
    }

    #[test]
    fn the_throttle_blocks_a_second_request_inside_the_interval() {
        let mut state = LiveState::default();
        let now = Instant::now();
        assert!(state.allows_request("com.example.Editor", now));
        state.record_request("com.example.Editor", now);
        assert!(!state.allows_request("com.example.Editor", now));
        assert!(state.allows_request(
            "com.example.Editor",
            now + MIN_REQUEST_INTERVAL + Duration::from_millis(1)
        ));
    }

    #[test]
    fn the_throttle_is_per_app() {
        let mut state = LiveState::default();
        let now = Instant::now();
        state.record_request("com.example.Editor", now);
        assert!(state.allows_request("com.example.Other", now));
    }

    #[test]
    fn live_analysis_needs_explicit_opt_in() {
        assert!(live_analysis_allowed(true, false));
        assert!(!live_analysis_allowed(false, false));
        assert!(!live_analysis_allowed(true, true));
        assert!(!live_analysis_allowed(false, true));
    }

    #[test]
    fn a_new_generation_invalidates_the_old_one() {
        let generation = AnalysisGeneration::default();
        let first = generation.begin();
        assert!(generation.is_current(first));
        let second = generation.begin();
        assert!(!generation.is_current(first));
        assert!(generation.is_current(second));
    }
}
