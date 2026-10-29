//! Start/stop facade over the in-process `"elo"` overlay.
//!
//! The overlay used to run as a child highlighter process; it now runs as a supervised thread in
//! this process (see [`crate::elo_overlay`]). This service keeps the tray, the settings commands,
//! and the restart watchdog talking to one small API.

use std::{io, sync::Arc};

/// Turns the overlay on and off and reports whether it is genuinely alive.
pub struct HighlighterService {
    /// The shared suggestion count, so the menu bar can show it.
    suggestion_count: Arc<crate::config::SuggestionCount>,
}

impl HighlighterService {
    /// Creates the service without starting anything.
    pub fn new(suggestion_count: Arc<crate::config::SuggestionCount>) -> Self {
        Self { suggestion_count }
    }

    /// Enables the overlay and respawns its loop thread if that thread is no longer alive.
    pub fn start(&self) -> io::Result<bool> {
        crate::elo_overlay::set_running(true);
        crate::elo_overlay::ensure_loop_alive()
            .map_err(|error| io::Error::other(format!("could not start the overlay: {error}")))?;
        Ok(self.is_running())
    }

    /// Disables the overlay. The loop thread keeps running idle so it can be re-enabled instantly.
    pub fn stop(&self) -> bool {
        crate::elo_overlay::set_running(false);
        false
    }

    /// How many suggestions the card currently has open.
    pub fn suggestion_count(&self) -> usize {
        self.suggestion_count.get()
    }

    /// Starts or stops the overlay based on the current state.
    ///
    /// The tray menu uses this as its single action for the highlighter service. Returns whether the
    /// service is running after the toggle completes.
    pub fn toggle(&self) -> io::Result<bool> {
        if self.is_running() {
            Ok(self.stop())
        } else {
            self.start()
        }
    }

    /// Whether the overlay is enabled *and* its loop thread has reported a recent heartbeat.
    ///
    /// A loop that panicked past its restart budget or is wedged stops heartbeating, so the
    /// watchdog sees it as stopped and can restart it.
    pub fn is_running(&self) -> bool {
        crate::elo_overlay::is_running() && crate::elo_overlay::is_loop_alive()
    }
}
