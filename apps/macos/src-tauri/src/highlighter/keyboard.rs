//! Keyboard navigation for the suggestion card.
//!
//! The plan specifies: "Keyboard: `⌃⌥G` focuses; arrows navigate; `Return`
//! accepts; `Esc` closes."
//!
//! The card window never takes keyboard focus — that is deliberate, because
//! activating the overlay window moves the caret and breaks the paste that
//! follows (PLAN.md §5.7, "Focus is sacred"). So the keyboard model cannot
//! be "the card receives key events".
//!
//! It works the way a system-wide assistant has to: the user drives it with
//! **global hotkeys**, and the highlighter keeps the navigation state. The
//! hotkeys are the ones the plan already names, so the muscle memory is the
//! same; only the modifier is one the app can claim without stealing focus.
//!
//! This module owns the navigation state: which suggestion is highlighted,
//! which row is shown, and what the next key does.

use crate::rect::ActionableLint;
use harper_core::linting::{Lint, Suggestion};

/// What the next key press should do.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    /// Nothing to do.
    None,
    /// Move to the next suggestion in the list.
    NextLint,
    /// Move to the previous suggestion.
    PreviousLint,
    /// Move to the next replacement row of the current suggestion.
    NextRow,
    /// Move to the previous replacement row.
    PreviousRow,
    /// Apply the shown replacement.
    Accept,
    /// Dismiss the current suggestion.
    Dismiss,
    /// Close the card.
    Close,
    /// Analyze the focused field.
    Analyze,
}

/// The keyboard navigation state.
#[derive(Debug, Default)]
pub struct KeyboardState {
    /// The suggestion the card is showing.
    highlighted: Option<usize>,
    /// The replacement row within that suggestion.
    row: Option<usize>,
}

impl KeyboardState {
    /// The suggestion the card is showing.
    pub fn highlighted(&self) -> Option<usize> {
        self.highlighted
    }

    /// The replacement row within that suggestion.
    pub fn row(&self) -> Option<usize> {
        self.row
    }

    /// Whether the card is showing anything.
    pub fn is_open(&self) -> bool {
        self.highlighted.is_some()
    }

    /// Open the card on the first suggestion.
    pub fn focus(&mut self, lint_count: usize) {
        if lint_count == 0 {
            self.highlighted = None;
            self.row = None;
            return;
        }
        self.highlighted = Some(0);
        self.row = None;
    }

    /// Close the card.
    pub fn close(&mut self) {
        self.highlighted = None;
        self.row = None;
    }

    /// How many replacement rows the suggestion has.
    fn row_count(lint: &Lint) -> usize {
        lint.suggestions.len().max(1)
    }

    /// Apply a key press.
    ///
    /// The lints are the current suggestions, so the state never points at a
    /// suggestion that has gone away.
    pub fn handle(&mut self, key: Key, lints: &[ActionableLint]) -> KeyAction {
        let Some(index) = self.highlighted else {
            // With no card open, the only meaningful key is the one that
            // opens it.
            return match key {
                Key::Analyze => KeyAction::Analyze,
                _ => KeyAction::None,
            };
        };

        let Some(lint) = lints.get(index) else {
            // The suggestion disappeared; close rather than act on stale state.
            self.close();
            return KeyAction::Close;
        };

        match key {
            Key::Next => {
                if let Some(row) = self.row {
                    let count = Self::row_count(&lint.lint);
                    if row + 1 < count {
                        self.row = Some(row + 1);
                    } else {
                        self.row = None;
                    }
                } else {
                    self.row = Some(0);
                }
                KeyAction::NextRow
            }
            Key::Previous => {
                match self.row {
                    Some(0) => self.row = None,
                    Some(row) => self.row = Some(row - 1),
                    None => {
                        // Step back to the previous suggestion.
                        self.highlighted = Some(index.saturating_sub(1));
                        self.row = None;
                    }
                }
                KeyAction::PreviousRow
            }
            Key::NextLint => {
                if index + 1 < lints.len() {
                    self.highlighted = Some(index + 1);
                    self.row = None;
                }
                KeyAction::NextLint
            }
            Key::PreviousLint => {
                if index > 0 {
                    self.highlighted = Some(index - 1);
                    self.row = None;
                }
                KeyAction::PreviousLint
            }
            Key::Accept => KeyAction::Accept,
            Key::Dismiss => KeyAction::Dismiss,
            Key::Close => {
                self.close();
                KeyAction::Close
            }
            Key::Analyze => KeyAction::Analyze,
        }
    }

    /// The replacement to apply, if the state points at one.
    pub fn selected_suggestion(&self, lints: &[ActionableLint]) -> Option<Suggestion> {
        let index = self.highlighted?;
        let lint = lints.get(index)?;
        match self.row {
            Some(row) => lint.lint.suggestions.get(row).cloned(),
            None => None,
        }
    }
}

/// The keys the navigation understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Move down.
    Next,
    /// Move up.
    Previous,
    /// Move to the next suggestion.
    NextLint,
    /// Move to the previous suggestion.
    PreviousLint,
    /// Apply.
    Accept,
    /// Dismiss.
    Dismiss,
    /// Close the card.
    Close,
    /// Analyze the field.
    Analyze,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rect::ActionableLint;
    use harper_core::linting::{Lint, LintKind, Suggestion};

    fn lint_with_suggestions(count: usize) -> Lint {
        Lint {
            span: harper_core::Span::new(0, 4),
            lint_kind: LintKind::Spelling,
            suggestions: (0..count)
                .map(|index| Suggestion::ReplaceWith(vec!['a'; index + 1]))
                .collect(),
            message: "test".to_string(),
            priority: 0,
        }
    }

    fn actionable(lint: Lint) -> ActionableLint {
        ActionableLint::new(
            crate::rect::Rect::new(0.0, 0.0, 10.0, 10.0),
            "Spelling".to_string(),
            lint,
            "text".to_string(),
            |_| {},
        )
    }

    #[test]
    fn focus_opens_the_first_suggestion() {
        let mut state = KeyboardState::default();
        state.focus(3);
        assert_eq!(state.highlighted(), Some(0));
        assert!(state.is_open());
    }

    #[test]
    fn focus_with_no_suggestions_does_nothing() {
        let mut state = KeyboardState::default();
        state.focus(0);
        assert!(!state.is_open());
    }

    #[test]
    fn down_moves_through_the_replacement_rows_then_out() {
        let lints = vec![actionable(lint_with_suggestions(2))];
        let mut state = KeyboardState::default();
        state.focus(1);

        assert_eq!(state.handle(Key::Next, &lints), KeyAction::NextRow);
        assert_eq!(state.row(), Some(0));
        assert_eq!(state.handle(Key::Next, &lints), KeyAction::NextRow);
        assert_eq!(state.row(), Some(1));
        // Past the last row the row selection clears.
        state.handle(Key::Next, &lints);
        assert_eq!(state.row(), None);
    }

    #[test]
    fn up_from_the_first_row_clears_it_and_up_again_moves_to_the_previous_suggestion() {
        let lints = vec![
            actionable(lint_with_suggestions(2)),
            actionable(lint_with_suggestions(2)),
        ];
        let mut state = KeyboardState::default();
        state.focus(2);
        state.highlighted = Some(1);

        state.handle(Key::Next, &lints);
        assert_eq!(state.row(), Some(0));
        state.handle(Key::Previous, &lints);
        assert_eq!(state.row(), None);
        state.handle(Key::Previous, &lints);
        assert_eq!(state.highlighted(), Some(0));
    }

    #[test]
    fn next_lint_moves_to_the_next_suggestion() {
        let lints = vec![
            actionable(lint_with_suggestions(1)),
            actionable(lint_with_suggestions(1)),
        ];
        let mut state = KeyboardState::default();
        state.focus(2);
        assert_eq!(state.handle(Key::NextLint, &lints), KeyAction::NextLint);
        assert_eq!(state.highlighted(), Some(1));
        // Past the last suggestion it stays.
        state.handle(Key::NextLint, &lints);
        assert_eq!(state.highlighted(), Some(1));
    }

    #[test]
    fn escape_closes_the_card() {
        let lints = vec![actionable(lint_with_suggestions(1))];
        let mut state = KeyboardState::default();
        state.focus(1);
        assert_eq!(state.handle(Key::Close, &lints), KeyAction::Close);
        assert!(!state.is_open());
    }

    #[test]
    fn accept_selects_the_shown_replacement() {
        let lints = vec![actionable(lint_with_suggestions(2))];
        let mut state = KeyboardState::default();
        state.focus(1);
        state.handle(Key::Next, &lints);

        let suggestion = state.selected_suggestion(&lints);
        assert!(suggestion.is_some());
    }

    #[test]
    fn with_no_row_selected_there_is_nothing_to_apply() {
        let lints = vec![actionable(lint_with_suggestions(2))];
        let mut state = KeyboardState::default();
        state.focus(1);
        assert!(state.selected_suggestion(&lints).is_none());
    }

    #[test]
    fn a_stale_suggestion_closes_the_card() {
        let mut state = KeyboardState::default();
        state.focus(2);
        // The suggestions went away.
        assert_eq!(state.handle(Key::Next, &[]), KeyAction::Close);
        assert!(!state.is_open());
    }

    #[test]
    fn with_no_card_open_only_analyze_does_something() {
        let mut state = KeyboardState::default();
        assert_eq!(state.handle(Key::Accept, &[]), KeyAction::None);
        assert_eq!(state.handle(Key::Analyze, &[]), KeyAction::Analyze);
    }
}
