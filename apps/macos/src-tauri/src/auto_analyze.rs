//! Opt-in "analyze as I write" watcher.
//!
//! It only runs when `auto_analyze` is enabled, waits for a typing pause, and
//! analyzes each distinct focused-field text at most once. The explicit widget
//! AI action and shortcuts remain available when this is off.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::config::Config;
use crate::os_broker::OsBroker;
use crate::{PlatformBroker, llm_analysis, llm_cache, main_thread};

const POLL_INTERVAL: Duration = Duration::from_millis(800);
const STABLE_DELAY: Duration = Duration::from_millis(1_400);

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_seen_hash = 0u64;
        let mut stable_since = std::time::Instant::now();
        let mut last_analyzed_hash = 0u64;

        loop {
            tokio::time::sleep(POLL_INTERVAL).await;

            let enabled = {
                let config = app.state::<Arc<Mutex<Config>>>();
                let config = config.lock().await;
                config.auto_analyze && !config.local_only
            };
            if !enabled {
                last_seen_hash = 0;
                continue;
            }

            let focused = main_thread::run(&app, |app| {
                let state = app.state::<RwLock<PlatformBroker>>();
                let mut broker = state
                    .write()
                    .map_err(|error| format!("Could not lock the platform broker: {error}"))?;
                broker.focused_text()
            });

            let Ok(Ok(Some(focused))) = focused else {
                last_seen_hash = 0;
                continue;
            };

            if focused.text.chars().count() < 20 {
                continue;
            }

            let hash = llm_cache::text_hash(&focused.text);
            if hash != last_seen_hash {
                last_seen_hash = hash;
                stable_since = std::time::Instant::now();
                continue;
            }

            if stable_since.elapsed() < STABLE_DELAY || hash == last_analyzed_hash {
                continue;
            }

            last_analyzed_hash = hash;
            if let Err(error) = llm_analysis::analyze_focused_field(&app).await {
                eprintln!("Automatic analysis skipped: {error}");
            }
        }
    });
}
