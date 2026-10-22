use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
#[cfg(target_os = "macos")]
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
use winit::window::WindowId;

use super::AddToDictionary;
use super::DisableRule;
use super::Error;
use super::IgnoreLint;
use super::RefreshConfig;
use super::render_state::{HitTarget, RenderState};
use super::window::Window;
use super::{AnalyzeField, OpenSettings};
use crate::os_broker::{LintText, OsBroker};
use crate::rect::ActionableLint;

const CONFIG_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// How often the highlighter asks the main process for a card command.
///
/// The card is a keyboard-driven surface, so the wait between a keypress and
/// the card moving has to be imperceptible. It is still a protocol round
/// trip, so it is not done every frame.
const CARD_POLL_INTERVAL: Duration = Duration::from_millis(40);

/// How often the highlighter asks the main process for the widget's position.
///
/// The pinned position changes when the user changes it, not per frame, so a
/// slower poll is enough.
const WIDGET_POLL_INTERVAL: Duration = Duration::from_millis(500);
const RECT_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Owns the winit event loop and the overlay windows created for each monitor.
///
/// `WindowManager` is intentionally separate from `Highlighter` because winit event-loop ownership
/// is a process-level concern. It also keeps monitor enumeration, native window lifecycle, and event
/// dispatch out of the public highlighter API.
pub struct WindowManager {
    event_loop: EventLoop<()>,
    rects: Vec<ActionableLint>,
    os_broker: Box<dyn OsBroker>,
    lint_text: LintText,
    ignore_lint: IgnoreLint,
    add_to_dictionary: AddToDictionary,
    disable_rule: DisableRule,
    /// Called when the user asks to see the request that would be sent.
    preview_request: crate::highlighter::PreviewRequest,
    refresh_config: RefreshConfig,
    open_settings: OpenSettings,
    analyze_field: AnalyzeField,
    /// Reports the widget anchor and tone for the app currently focused.
    widget_settings: WidgetSettings,
    /// Reports the card command the main process asked for, if any.
    card_command: CardCommandFn,
    /// Fetches the pending card command, if there is one.
    fetch_card_command: FetchCardCommandFn,
    /// Reports the card's state back to the main process.
    report_state: ReportStateFn,
}

pub struct WindowManagerCallbacks {
    pub lint_text: LintText,
    pub ignore_lint: IgnoreLint,
    pub add_to_dictionary: AddToDictionary,
    pub disable_rule: DisableRule,
    /// Called when the user asks to see the request that would be sent.
    pub preview_request: crate::highlighter::PreviewRequest,
    pub refresh_config: RefreshConfig,
    pub open_settings: OpenSettings,
    pub analyze_field: AnalyzeField,
    /// Reports the widget anchor and tone for the app currently focused.
    pub widget_settings: WidgetSettings,
    /// Reports the card command the main process asked for, if any.
    pub card_command: CardCommandFn,
    /// Fetches the pending card command, if there is one.
    ///
    /// This is polled for often when the card is open, so it is a separate,
    /// cheap channel from the configuration poll.
    pub fetch_card_command: FetchCardCommandFn,
    /// Reports the card's state back to the main process.
    pub report_state: ReportStateFn,
}

/// Fetches a pending card command from the main process.
pub type FetchCardCommandFn = Box<dyn FnMut() -> Option<crate::config::CardCommand> + 'static>;

/// Reports the card's state to the main process.
pub type ReportStateFn = Box<dyn FnMut(usize) + 'static>;

/// The pending card command, if the main process asked for one.
pub type CardCommandFn = Box<dyn FnMut() -> Option<crate::config::CardCommand> + 'static>;

/// The widget settings for the app currently focused.
///
/// The highlighter passes the focused bundle identifier, because it is the only
/// process that knows which app is in front.
pub type WidgetSettings = Box<dyn FnMut(&str) -> Option<crate::widget_anchor::WidgetAnchor>>;

impl WindowManager {
    /// Creates the event loop before windows exist because winit requires window creation to happen
    /// from inside that loop's lifecycle callbacks.
    pub fn new(
        os_broker: Box<dyn OsBroker>,
        callbacks: WindowManagerCallbacks,
    ) -> Result<Self, Error> {
        let mut event_loop_builder = EventLoop::builder();

        #[cfg(target_os = "macos")]
        event_loop_builder
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_default_menu(false);

        Ok(Self {
            event_loop: event_loop_builder.build()?,
            rects: Vec::new(),
            os_broker,
            lint_text: callbacks.lint_text,
            ignore_lint: callbacks.ignore_lint,
            add_to_dictionary: callbacks.add_to_dictionary,
            disable_rule: callbacks.disable_rule,
            preview_request: callbacks.preview_request,
            refresh_config: callbacks.refresh_config,
            open_settings: callbacks.open_settings,
            analyze_field: callbacks.analyze_field,
            widget_settings: callbacks.widget_settings,
            card_command: callbacks.card_command,
            fetch_card_command: callbacks.fetch_card_command,
            report_state: callbacks.report_state,
        })
    }

    /// Seeds externally supplied lint rectangles before the event loop takes ownership of rendering.
    pub fn set_rects(&mut self, rects: Vec<ActionableLint>) {
        self.rects = rects;
    }

    /// Transfers ownership into winit's application handler because `run_app` owns the process-level
    /// event loop until the overlay exits.
    pub fn run_window_for_each_monitor(self) -> Result<(), Error> {
        let mut app = WindowManagerApp::new(
            self.rects,
            self.os_broker,
            WindowManagerCallbacks {
                lint_text: self.lint_text,
                ignore_lint: self.ignore_lint,
                add_to_dictionary: self.add_to_dictionary,
                disable_rule: self.disable_rule,
                preview_request: self.preview_request,
                refresh_config: self.refresh_config,
                open_settings: self.open_settings,
                analyze_field: self.analyze_field,
                widget_settings: self.widget_settings,
                card_command: self.card_command,
                fetch_card_command: self.fetch_card_command,
                report_state: self.report_state,
            },
        );

        // Run continuously without a read timer; VSync presentation provides backpressure.
        self.event_loop.set_control_flow(ControlFlow::Poll);
        let result = self.event_loop.run_app(&mut app);

        if let Some(error) = app.error {
            return Err(error);
        }

        result.map_err(Error::from)
    }
}

struct WindowManagerApp {
    windows: Vec<Window>,
    render_state: RenderState,
    os_broker: Box<dyn OsBroker>,
    lint_text: LintText,
    last_config_poll: Instant,
    refresh_config: RefreshConfig,
    hovered_lint: Option<usize>,
    cursor_hittest_enabled: bool,
    error: Option<Error>,
    widget_settings: WidgetSettings,
    card_command: CardCommandFn,
    fetch_card_command: FetchCardCommandFn,
    report_state: ReportStateFn,
    last_card_poll: Instant,
    last_widget_poll: Instant,
    last_rect_poll: Instant,
    /// The app the widget's anchor was last fetched for.
    last_anchor_app: Option<String>,
    last_reported_count: Option<usize>,
}

impl WindowManagerApp {
    /// Builds the mutable application state consumed by winit callbacks after `WindowManager` gives
    /// up direct control of the event loop.
    fn new(
        rects: Vec<ActionableLint>,
        os_broker: Box<dyn OsBroker>,
        callbacks: WindowManagerCallbacks,
    ) -> Self {
        let now = Instant::now();
        Self {
            windows: Vec::new(),
            render_state: RenderState::new(
                rects,
                callbacks.ignore_lint,
                callbacks.add_to_dictionary,
                callbacks.disable_rule,
                callbacks.preview_request,
                callbacks.open_settings,
                callbacks.analyze_field,
            ),
            os_broker,
            lint_text: callbacks.lint_text,
            last_config_poll: now,
            refresh_config: callbacks.refresh_config,
            hovered_lint: None,
            cursor_hittest_enabled: false,
            error: None,
            widget_settings: callbacks.widget_settings,
            card_command: callbacks.card_command,
            fetch_card_command: callbacks.fetch_card_command,
            report_state: callbacks.report_state,
            last_card_poll: now,
            last_widget_poll: now.checked_sub(WIDGET_POLL_INTERVAL).unwrap_or(now),
            last_rect_poll: now.checked_sub(RECT_POLL_INTERVAL).unwrap_or(now),
            last_anchor_app: None,
            last_reported_count: None,
        }
    }

    /// Refreshes lint geometry from the OS broker inside the event loop so repaint requests happen on
    /// the same thread that owns the overlay windows.
    fn read_rect_updates(&mut self) {
        let lints = self.os_broker.get_boxes(self.lint_text.as_mut());
        let field_active = self.os_broker.has_active_field();
        let field_rect = self.os_broker.active_field_rect();
        match lints {
            Some(lints) => {
                let has_lints = !lints.is_empty();
                self.render_state
                    .set_field_active(field_active || has_lints);
                self.render_state.set_field_rect(field_rect);
                self.render_state.set_lints(lints);
                if has_lints
                    && !self.render_state.has_active_popup()
                    && !self.render_state.was_dismissed_by_user()
                {
                    self.render_state.open_widget();
                }
            }
            None => {
                if !field_active && self.render_state.open_suggestion_count() == 0 {
                    self.render_state.set_field_active(false);
                    self.render_state.set_field_rect(None);
                }
            }
        }

        // A card command from the main process: the keyboard shortcuts live
        // there, and the card lives here.
        if let Some(command) = (self.card_command)() {
            self.apply_card_command(command);
        }

        // The pinned position belongs to the focused app and comes from the
        // parent process. The tone is detected locally while the tree is
        // walked, so it costs nothing and works offline.
        let now = Instant::now();
        if let Ok(Some(bundle_id)) = self.os_broker.focused_bundle_id() {
            let app_changed = self.last_anchor_app.as_deref() != Some(bundle_id.as_str());
            if app_changed || now.duration_since(self.last_widget_poll) >= WIDGET_POLL_INTERVAL {
                let anchor = (self.widget_settings)(&bundle_id);
                self.render_state.set_widget_anchor(anchor);
                self.last_anchor_app = Some(bundle_id);
                self.last_widget_poll = now;
            }
        }
        self.render_state
            .set_tone_badge(self.os_broker.detected_tone());

        let count = self.render_state.open_suggestion_count();
        if self.last_reported_count != Some(count) {
            self.last_reported_count = Some(count);
            (self.report_state)(count);
        }

        for window in &self.windows {
            window.request_redraw();
        }
    }

    fn refresh_config(&mut self) {
        (self.refresh_config)();
    }

    /// Applies a command for the suggestion card.
    fn apply_card_command(&mut self, command: crate::config::CardCommand) {
        use crate::config::CardCommand;

        match command {
            CardCommand::Focus => self.render_state.open_widget(),
            CardCommand::NextSuggestion => self.render_state.next_lint(),
            CardCommand::PreviousSuggestion => self.render_state.previous_lint(),
            CardCommand::NextReplacement => self.render_state.next_suggestion_row(),
            CardCommand::PreviousReplacement => self.render_state.previous_suggestion_row(),
            CardCommand::Accept => {
                self.render_state.apply_shown_replacement();
            }
            CardCommand::Dismiss => {
                self.render_state.dismiss_shown_lint();
            }
            CardCommand::Close => self.render_state.close_popup(),
        }

        // The main process shows how many suggestions the card has, so the
        // state is reported back after every change.
        self.report_card_state();
        for window in &self.windows {
            window.request_redraw();
        }
    }

    /// Reports what the card is showing to the main process.
    fn report_card_state(&mut self) {
        let suggestions = self.render_state.open_suggestion_count();
        self.last_reported_count = Some(suggestions);
        (self.report_state)(suggestions);
    }

    /// Toggles native click-through behavior based on the cursor's current interactive Elocutio target,
    /// keeping editor clicks from being swallowed when the pointer is outside highlights and popups.
    fn update_cursor_hittest(&mut self, event_loop: &ActiveEventLoop) {
        let Some(cursor_pos) = self.os_broker.cursor_position() else {
            return;
        };

        let hit_target = self.render_state.hit_target_at_pos(cursor_pos);
        let prev_hovered = self.hovered_lint;

        self.hovered_lint = match hit_target {
            HitTarget::Lint(index) => Some(index),
            HitTarget::Widget | HitTarget::Analyze | HitTarget::Popup | HitTarget::None => None,
        };

        if self.hovered_lint != prev_hovered
            && let Some(index) = self.hovered_lint
            && !self.render_state.has_active_popup()
        {
            self.render_state.set_highlighted_lint(Some(index));
            for window in &self.windows {
                window.request_redraw();
            }
        }

        let should_enable_hittest = !matches!(hit_target, HitTarget::None);

        if self.cursor_hittest_enabled == should_enable_hittest {
            return;
        }

        for window in &self.windows {
            if let Err(error) = window.set_cursor_hittest(should_enable_hittest) {
                self.error = Some(error);
                event_loop.exit();
                return;
            }
        }

        self.cursor_hittest_enabled = should_enable_hittest;
    }

    /// Opens or replaces the popup only when the actual click target is a lint, leaving popup clicks
    /// for egui controls such as the close button.
    fn select_hit_lint(&mut self) {
        let Some(cursor_pos) = self.os_broker.cursor_position() else {
            return;
        };

        match self.render_state.hit_target_at_pos(cursor_pos) {
            HitTarget::Lint(index) => {
                self.render_state.set_highlighted_lint(Some(index));
            }
            HitTarget::Widget => self.render_state.open_widget(),
            HitTarget::Analyze => self.render_state.request_analyze(),
            HitTarget::Popup | HitTarget::None => return,
        }

        for window in &self.windows {
            window.request_redraw();
        }
    }
}

impl ApplicationHandler for WindowManagerApp {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();

        if now.duration_since(self.last_rect_poll) >= RECT_POLL_INTERVAL {
            self.last_rect_poll = now;
            self.read_rect_updates();
        }

        if now.duration_since(self.last_config_poll) >= CONFIG_POLL_INTERVAL {
            self.refresh_config();
            self.last_config_poll = now;
        }

        // The card is polled for a command more often than the configuration
        // is, because a keypress has to feel immediate.
        if now.duration_since(self.last_card_poll) >= CARD_POLL_INTERVAL {
            self.last_card_poll = now;
            if let Some(command) = (self.fetch_card_command)() {
                self.apply_card_command(command);
            }
        }

        self.update_cursor_hittest(event_loop);

        // Wake regularly to poll card commands, cursor hit-testing, and rect updates.
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + CARD_POLL_INTERVAL));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }

        let monitors = event_loop.available_monitors().collect::<Vec<_>>();

        for monitor in monitors {
            match pollster::block_on(Window::new(event_loop, monitor)) {
                Ok(window) => self.windows.push(window),
                Err(error) => {
                    self.error = Some(error);
                    event_loop.exit();
                    return;
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let should_select_hit_lint = matches!(
            &event,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            }
        );
        let should_render = matches!(&event, WindowEvent::RedrawRequested);

        if let Some(window) = self
            .windows
            .iter_mut()
            .find(|window| window.id() == window_id)
        {
            window.handle_event(&event);

            if should_render {
                window.render(&mut self.render_state);
            }
        }

        if should_select_hit_lint {
            self.select_hit_lint();
        }

        if matches!(&event, WindowEvent::CloseRequested) {
            event_loop.exit();
        }
    }
}
