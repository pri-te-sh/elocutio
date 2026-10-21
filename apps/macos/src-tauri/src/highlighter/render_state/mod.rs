mod draw_highlight;
mod popup_layout;
mod render_lint_card;

use std::time::Instant;

use egui_commonmark::CommonMarkCache;
use harper_core::{Document, linting::LintKind};

use self::{
    draw_highlight::draw_highlight,
    popup_layout::{PopupLayout, approach},
    render_lint_card::{
        FIRST_ROW_OFFSET, LintCardAction, popup_layout_for_lint, render_lint_card, row_count,
        sidebar_target,
    },
};
use super::{AddToDictionary, AnalyzeField, DisableRule, IgnoreLint, OpenSettings};
use crate::{
    lint_kind_color::lint_kind_color,
    rect::{ActionableLint, Rect},
};

/// A lint marker, popup, or empty overlay region found during cursor hit-testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    Lint(usize),
    Popup,
    Widget,
    Analyze,
    None,
}

/// Stores highlighter-specific drawing state and renders it into an egui frame.
///
/// `Window` owns the native window and GPU plumbing; `RenderState` owns the content we want to draw
/// into that plumbing. This keeps future highlight rectangles, styling, and animation state out of
/// the platform/rendering infrastructure.
pub struct RenderState {
    /// Last successful accessibility read, retained when a later read fails.
    last_lints: Option<Vec<ActionableLint>>,

    /// Index of the lint whose suggestion popup is currently visible.
    highlighted_lint: Option<usize>,

    /// Index of the suggestion row that the user hovered last in the visible popup.
    shown_suggestion: Option<usize>,

    /// Screen-space bounds of the floating suggestion-count widget, if drawn.
    widget_rect: Option<egui::Rect>,

    /// Screen-space bounds of the widget's "AI" action zone.
    analyze_rect: Option<egui::Rect>,

    /// Where the widget is anchored for the current app.
    ///
    /// The user can pin the widget to a corner per app, and the position
    /// persists. `None` means the default: beside the first underline, or in
    /// the bottom-right corner when there is nothing to underline.
    widget_anchor: Option<crate::widget_anchor::WidgetAnchor>,

    /// The tone detected for the current field, shown as a badge on the widget.
    tone_badge: Option<String>,

    /// Whether a supported editable field is currently focused.
    field_active: bool,

    /// The flyout card's drawn offset from the list card's top, in points.
    current_sidebar_position: f32,

    /// When `render` last ran, used to move the sidebar at a speed that follows real time.
    last_render: Option<Instant>,

    /// Cache for markdown-rendered lint messages so popup redraws do not repeatedly rebuild markdown
    /// resources from scratch.
    markdown_cache: CommonMarkCache,

    /// Called when the user dismisses a lint so app-level ignored-lint state can be updated.
    ignore_lint: IgnoreLint,

    /// Called when the user adds a spelling lint's source text to the local dictionary.
    add_to_dictionary: AddToDictionary,

    /// Called when the user disables the rule that produced the selected lint.
    disable_rule: DisableRule,

    open_settings: OpenSettings,
    analyze_field: AnalyzeField,
}

impl RenderState {
    /// Creates render state through `set_lints` so initial lint data gets the same stale-popup guard
    /// used by later accessibility refreshes.
    pub fn new(
        rects: Vec<ActionableLint>,
        ignore_lint: IgnoreLint,
        add_to_dictionary: AddToDictionary,
        disable_rule: DisableRule,
        open_settings: OpenSettings,
        analyze_field: AnalyzeField,
    ) -> Self {
        let mut state = Self {
            last_lints: None,
            highlighted_lint: None,
            shown_suggestion: None,
            current_sidebar_position: FIRST_ROW_OFFSET,
            widget_rect: None,
            analyze_rect: None,
            widget_anchor: None,
            tone_badge: None,
            field_active: false,
            last_render: None,
            markdown_cache: CommonMarkCache::default(),
            ignore_lint,
            add_to_dictionary,
            disable_rule,
            open_settings,
            analyze_field,
        };
        state.set_lints(rects);
        state
    }

    /// Saves successful accessibility reads while retaining the previous lints after a failed read.
    ///
    /// Closes the popup when its lint disappeared, and forgets the shown suggestion when the
    /// refreshed lint has fewer rows.
    pub fn set_lints(&mut self, lints: Vec<ActionableLint>) {
        self.last_lints = Some(lints);

        if self
            .highlighted_lint
            .is_some_and(|index| index >= self.lints().len())
        {
            self.close_popup();
        }

        let row_count = self
            .highlighted_lint
            .and_then(|index| self.lints().get(index))
            .map(|positioned_lint| row_count(&positioned_lint.lint));
        self.shown_suggestion = self
            .shown_suggestion
            .filter(|index| row_count.is_some_and(|row_count| *index < row_count));
    }

    pub fn set_field_active(&mut self, active: bool) {
        self.field_active = active;
    }

    /// Pin the widget for the current app, or return it to the default.
    pub fn set_widget_anchor(&mut self, anchor: Option<crate::widget_anchor::WidgetAnchor>) {
        self.widget_anchor = anchor;
    }

    /// The tone detected for the current field, shown as a badge.
    pub fn set_tone_badge(&mut self, tone: Option<String>) {
        self.tone_badge = tone;
    }

    fn lints(&self) -> &[ActionableLint] {
        self.last_lints.as_deref().unwrap_or_default()
    }

    /// Updates which lint owns the suggestion popup without exposing render-state internals.
    ///
    /// Window/input code decides what the user interacted with, while `RenderState` owns the popup
    /// selection. Filtering invalid indexes here keeps stale cursor state from selecting a lint that
    /// disappeared after the latest accessibility read.
    ///
    /// Like the Chrome extension, each new selection starts again from the first suggestion row.
    pub fn set_highlighted_lint(&mut self, highlighted_lint: Option<usize>) {
        self.highlighted_lint = highlighted_lint.filter(|index| *index < self.lints().len());
        self.shown_suggestion = None;
        self.current_sidebar_position = FIRST_ROW_OFFSET;
    }

    /// Centralizes the close behavior so the popup close button only has to clear the selected lint.
    pub fn open_widget(&mut self) {
        if !self.lints().is_empty() {
            self.set_highlighted_lint(Some(0));
        }
    }

    pub fn request_analyze(&mut self) {
        (self.analyze_field)();
    }

    pub fn close_popup(&mut self) {
        self.highlighted_lint = None;
        self.shown_suggestion = None;
        self.current_sidebar_position = FIRST_ROW_OFFSET;
    }

    /// Checks if a given position touches a hit target.
    pub fn hit_target_at_pos(&self, pos: egui::Pos2) -> HitTarget {
        if self.analyze_rect.is_some_and(|rect| rect.contains(pos)) {
            return HitTarget::Analyze;
        }

        if self.widget_rect.is_some_and(|rect| rect.contains(pos)) {
            return HitTarget::Widget;
        }

        if self
            .popup_layout()
            .is_some_and(|layout| layout.contains(pos))
        {
            return HitTarget::Popup;
        }

        self.lints()
            .iter()
            .position(|positioned_lint| {
                rect_bounds(&positioned_lint.rect.with_friendly_padding()).contains(pos)
            })
            .map_or(HitTarget::None, HitTarget::Lint)
    }

    /// Computes popup hit-test bounds from our layout contract instead of waiting for egui to report
    /// rendered bounds, which keeps hit-testing available before the next render pass completes.
    pub fn popup_layout(&self) -> Option<PopupLayout> {
        self.highlighted_lint
            .and_then(|index| self.lints().get(index))
            .map(|positioned_lint| {
                popup_layout_for_lint(
                    &positioned_lint.rect,
                    &positioned_lint.lint,
                    self.current_sidebar_position,
                )
            })
    }

    /// Draws highlights and the active popup from the same state used by hit-testing so visible
    /// regions and clickable regions do not drift apart.
    ///
    /// Each call also moves the sidebar toward its target by the time since the previous call.
    pub fn render(&mut self, ui: &mut egui::Ui) {
        let now = Instant::now();
        let time_since_last_render = self
            .last_render
            .map_or(0.0, |last_render| (now - last_render).as_secs_f32());
        self.last_render = Some(now);

        if let Some(positioned_lint) = self
            .highlighted_lint
            .and_then(|index| self.lints().get(index))
        {
            let target = sidebar_target(&positioned_lint.lint, self.shown_suggestion);
            self.current_sidebar_position = approach(
                self.current_sidebar_position,
                target,
                time_since_last_render,
            );
        }

        for positioned_lint in self.lints() {
            draw_highlight(ui, &positioned_lint.rect, &positioned_lint.lint);
        }

        if let Some(index) = self.highlighted_lint
            && let Some(positioned_lint) = self.lints().get(index)
        {
            let rect = positioned_lint.rect;
            let lint = positioned_lint.lint.clone();
            let source_text = positioned_lint.source_text.clone();

            match render_lint_card(
                ui,
                &rect,
                &lint,
                &source_text,
                self.shown_suggestion,
                self.current_sidebar_position,
                &mut self.markdown_cache,
            ) {
                Some(LintCardAction::Close) => self.close_popup(),
                Some(LintCardAction::OpenSettings) => {
                    (self.open_settings)();
                    self.close_popup();
                }
                Some(LintCardAction::HoverSuggestion(suggestion)) => {
                    self.shown_suggestion = Some(suggestion);
                }
                Some(LintCardAction::ApplySuggestion(suggestion)) => {
                    if let Some(actionable_lint) = self
                        .last_lints
                        .as_mut()
                        .and_then(|lints| lints.get_mut(index))
                    {
                        record_interaction(
                            actionable_lint,
                            crate::events::EventKind::Accepted,
                            &suggestion,
                        );
                        actionable_lint.apply_suggestion(suggestion);
                    }

                    self.close_popup();
                }
                Some(LintCardAction::IgnoreLint) => {
                    if let Some((lint, source_text)) =
                        self.lints().get(index).map(|actionable_lint| {
                            (
                                actionable_lint.lint.clone(),
                                actionable_lint.source_text.clone(),
                            )
                        })
                    {
                        record_interaction(
                            &self.lints()[index],
                            crate::events::EventKind::Dismissed,
                            &harper_core::linting::Suggestion::Remove,
                        );

                        // Dismissing is a negative preference signal, so the
                        // voice profile learns from it too. The call is a
                        // no-op when voice learning is off.
                        let actionable = &self.lints()[index];
                        let original = lint.get_str(&source_text.chars().collect::<Vec<_>>());
                        let category = actionable
                            .rule_name
                            .strip_prefix("llm:")
                            .unwrap_or(&actionable.rule_name)
                            .to_string();
                        if let Err(error) = crate::voice::record_dismissed(&original, "", &category)
                        {
                            eprintln!("Could not record the dismissed edit: {error}");
                        }

                        let document = Document::new_markdown_default_curated(&source_text);
                        (self.ignore_lint)(&lint, &document);
                    }

                    self.close_popup();
                }
                Some(LintCardAction::AddToDictionary) => {
                    if let Some((lint, source_text)) =
                        self.lints().get(index).map(|actionable_lint| {
                            (
                                actionable_lint.lint.clone(),
                                actionable_lint.source_text.clone(),
                            )
                        })
                    {
                        let document = Document::new_markdown_default_curated(&source_text);
                        let word = lint.get_str(document.get_source());
                        (self.add_to_dictionary)(&word);
                    }

                    self.close_popup();
                }
                Some(LintCardAction::DisableRule) => {
                    if let Some(rule_name) = self
                        .lints()
                        .get(index)
                        .map(|actionable_lint| actionable_lint.rule_name.clone())
                    {
                        (self.disable_rule)(&rule_name);
                    }

                    self.close_popup();
                }
                None => {}
            }
        }

        if self.field_active {
            let size = egui::vec2(148.0, 36.0);
            let screen = ui.ctx().content_rect();
            // A pinned anchor wins. Otherwise the widget sits beside the first
            // underline, or in the bottom-right corner when there is nothing
            // to underline.
            let mut pos = match self.widget_anchor {
                Some(anchor) => anchor.position(screen, size),
                None => {
                    if let Some(first) = self.lints().first() {
                        let anchor = rect_bounds(&first.rect);
                        let above = anchor.top() - size.y - 6.0;
                        let y = if above >= screen.top() + 8.0 {
                            above
                        } else {
                            anchor.bottom() + 6.0
                        };
                        egui::pos2(anchor.left(), y)
                    } else {
                        egui::pos2(
                            screen.right() - size.x - 24.0,
                            screen.bottom() - size.y - 24.0,
                        )
                    }
                }
            };
            if pos.x + size.x > screen.right() - 8.0 {
                pos.x = screen.right() - size.x - 8.0;
            }
            pos.x = pos.x.max(screen.left() + 8.0);
            pos.y = pos
                .y
                .clamp(screen.top() + 8.0, screen.bottom() - size.y - 8.0);
            let rect = egui::Rect::from_min_size(pos, size);
            self.widget_rect = Some(rect);
            let analyze_rect = egui::Rect::from_min_max(
                egui::pos2(rect.right() - 46.0, rect.top()),
                rect.right_bottom(),
            );
            self.analyze_rect = Some(analyze_rect);

            let response = ui.interact(
                rect,
                ui.id().with("elocutio-suggestion-widget"),
                egui::Sense::click(),
            );
            let analyze_response = ui.interact(
                analyze_rect,
                ui.id().with("elocutio-analyze-widget"),
                egui::Sense::click(),
            );
            let fill = if response.hovered() {
                egui::Color32::from_rgb(0xf2, 0xf3, 0xff)
            } else {
                egui::Color32::WHITE
            };
            let border = egui::Color32::from_rgb(0xd0, 0xd7, 0xde);
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(8), fill);
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(8),
                egui::Stroke::new(1.0_f32, border),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                egui::pos2(rect.left() + 14.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                "\"",
                egui::FontId::proportional(24.0),
                egui::Color32::from_rgb(0x19, 0x1f, 0x70),
            );
            ui.painter().text(
                egui::pos2(analyze_rect.left() - 12.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                self.lints().len().to_string(),
                egui::FontId::proportional(14.0),
                egui::Color32::from_rgb(0x1f, 0x23, 0x28),
            );
            if analyze_response.hovered() {
                ui.painter()
                    .rect_filled(analyze_rect, egui::CornerRadius::same(8), fill);
                ui.painter().rect_stroke(
                    analyze_rect,
                    egui::CornerRadius::same(8),
                    egui::Stroke::new(1.0_f32, border),
                    egui::StrokeKind::Inside,
                );
            }
            ui.painter().text(
                analyze_rect.center(),
                egui::Align2::CENTER_CENTER,
                "AI",
                egui::FontId::proportional(13.0),
                egui::Color32::from_rgb(0x19, 0x1f, 0x70),
            );

            // The tone badge sits between the count and the AI action, and
            // only when a tone was actually detected.
            if let Some(tone) = &self.tone_badge {
                let tone_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 46.0, rect.top() + 8.0),
                    egui::pos2(rect.left() + 96.0, rect.bottom() - 8.0),
                );
                ui.painter().text(
                    tone_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    tone,
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_rgb(0x4a, 0x52, 0x66),
                );
            }

            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
                && !analyze_response.clicked()
            {
                self.open_widget();
            }
        } else {
            self.widget_rect = None;
            self.analyze_rect = None;
        }
    }
}

/// Records an interaction with a suggestion for the local event log.
///
/// The event carries sizes, not text: the user's document content never enters
/// the log. Recording is opt-in, so this is a no-op unless the user turned it
/// on.
fn record_interaction(
    actionable_lint: &ActionableLint,
    kind: crate::events::EventKind,
    suggestion: &harper_core::linting::Suggestion,
) {
    let original_length = actionable_lint.lint.span.len();
    let replacement_length = match suggestion {
        harper_core::linting::Suggestion::ReplaceWith(chars) => chars.len(),
        harper_core::linting::Suggestion::InsertAfter(chars) => chars.len(),
        harper_core::linting::Suggestion::Remove => 0,
    };
    let origin = if actionable_lint.rule_name.starts_with("llm:") {
        crate::events::Origin::Model
    } else {
        crate::events::Origin::Local
    };
    let category = actionable_lint
        .rule_name
        .strip_prefix("llm:")
        .unwrap_or(&actionable_lint.rule_name)
        .to_string();

    if let Err(error) = crate::events::record_event(
        kind,
        origin,
        &actionable_lint.rule_name,
        &category,
        original_length,
        replacement_length,
    ) {
        eprintln!("Could not record the suggestion event: {error}");
    }
}

/// Converts accessibility rectangles into egui rectangles so rendering and hit-testing use the same
/// screen-space geometry.
fn rect_bounds(rect: &Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(rect.x as f32, rect.y as f32),
        egui::vec2(rect.width as f32, rect.height as f32),
    )
}

fn lint_kind_color32(lint_kind: LintKind) -> egui::Color32 {
    let color = lint_kind_color(lint_kind);

    egui::Color32::from_rgb(color.r, color.g, color.b)
}
