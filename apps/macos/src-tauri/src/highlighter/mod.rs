mod error;
pub mod keyboard;
mod render_state;
mod window;
mod window_manager;

use std::collections::BTreeMap;

pub use error::Error;
use window_manager::{WindowManager, WindowManagerCallbacks};

use harper_core::{Document, linting::Lint};

use crate::os_broker::{LintText, OsBroker};
use crate::rect::ActionableLint;

type IgnoreLint = Box<dyn FnMut(&Lint, &Document)>;
type AddToDictionary = Box<dyn FnMut(&str)>;
type DisableRule = Box<dyn FnMut(&str)>;
type RefreshConfig = Box<dyn FnMut()>;
type OpenSettings = Box<dyn FnMut()>;
pub type AnalyzeField = Box<dyn FnMut()>;

/// Public entry point for the screen highlighter system.
///
/// `Highlighter` delegates native window/event-loop work to the window manager. Keeping this as the
/// top-level type gives future callers one place to configure or update highlighter state without
/// depending on the windowing implementation.
pub struct Highlighter {
    window_manager: WindowManager,
}

impl Highlighter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        os_broker: impl OsBroker + 'static,
        lint_text: impl FnMut(&str) -> BTreeMap<String, Vec<Lint>> + 'static,
        ignore_lint: impl FnMut(&Lint, &Document) + 'static,
        add_to_dictionary: impl FnMut(&str) + 'static,
        disable_rule: impl FnMut(&str) + 'static,
        refresh_config: impl FnMut() + 'static,
        open_settings: impl FnMut() + 'static,
        analyze_field: impl FnMut() + 'static,
        widget_settings: impl FnMut(&str) -> Option<crate::widget_anchor::WidgetAnchor> + 'static,
        card_command: impl FnMut() -> Option<crate::config::CardCommand> + 'static,
        fetch_card_command: impl FnMut() -> Option<crate::config::CardCommand> + 'static,
        report_state: impl FnMut(usize) + 'static,
    ) -> Result<Self, Error> {
        let lint_text: LintText = Box::new(lint_text);
        let ignore_lint: IgnoreLint = Box::new(ignore_lint);
        let add_to_dictionary: AddToDictionary = Box::new(add_to_dictionary);
        let disable_rule: DisableRule = Box::new(disable_rule);
        let refresh_config: RefreshConfig = Box::new(refresh_config);
        let open_settings: OpenSettings = Box::new(open_settings);
        let analyze_field: AnalyzeField = Box::new(analyze_field);

        Ok(Self {
            window_manager: WindowManager::new(
                Box::new(os_broker),
                WindowManagerCallbacks {
                    lint_text,
                    ignore_lint,
                    add_to_dictionary,
                    disable_rule,
                    refresh_config,
                    open_settings,
                    analyze_field,
                    widget_settings: Box::new(widget_settings),
                    card_command: Box::new(card_command),
                    fetch_card_command: Box::new(fetch_card_command),
                    report_state: Box::new(report_state),
                },
            )?,
        })
    }

    pub fn run_window_for_each_monitor(self) -> Result<(), Error> {
        self.window_manager.run_window_for_each_monitor()
    }

    pub fn set_rects(&mut self, rects: Vec<ActionableLint>) {
        self.window_manager.set_rects(rects);
    }
}
