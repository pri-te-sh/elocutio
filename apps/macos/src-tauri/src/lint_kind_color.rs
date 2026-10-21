use harper_core::linting::LintKind;

use crate::color::Color;

pub const LINT_KINDS: [LintKind; 20] = [
    LintKind::Agreement,
    LintKind::BoundaryError,
    LintKind::Capitalization,
    LintKind::Eggcorn,
    LintKind::Enhancement,
    LintKind::Formatting,
    LintKind::Grammar,
    LintKind::Malapropism,
    LintKind::Miscellaneous,
    LintKind::Nonstandard,
    LintKind::Punctuation,
    LintKind::Readability,
    LintKind::Redundancy,
    LintKind::Regionalism,
    LintKind::Repetition,
    LintKind::Spelling,
    LintKind::Style,
    LintKind::Typo,
    LintKind::Usage,
    LintKind::WordChoice,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextColor {
    Black,
    White,
}

/// The three underline colours the plan calls for.
///
/// "red/blue/gray underlines immediately" (PLAN.md §4.2). Three colours, not
/// twenty: a user learns what red means in one glance, and the overlay stays
/// quiet enough to read through. The inherited Harper palette used a different
/// colour for every rule kind, which is why the mapping is a category rather
/// than a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderlineKind {
    /// A correctness problem: spelling, grammar, punctuation.
    Red,
    /// A style or clarity problem: concision, word choice, tone.
    Blue,
    /// Something worth knowing about but not worth changing.
    Gray,
}

impl UnderlineKind {
    /// The colour the underline is drawn in.
    pub const fn color(self) -> Color {
        match self {
            // A warm red for correctness, close to what a reader expects.
            UnderlineKind::Red => Color::new(0xD6, 0x3A, 0x4B),
            // Ink Indigo for style, which is the brand's primary.
            UnderlineKind::Blue => Color::new(0x3D, 0x4C, 0xB8),
            // A neutral gray for advisory notes.
            UnderlineKind::Gray => Color::new(0x8A, 0x8F, 0x9C),
        }
    }
}

/// Which of the three underline colours a lint kind uses.
pub fn underline_kind(lint_kind: LintKind) -> UnderlineKind {
    match lint_kind {
        // Correctness.
        LintKind::Spelling
        | LintKind::Typo
        | LintKind::Capitalization
        | LintKind::Punctuation
        | LintKind::Agreement
        | LintKind::BoundaryError
        | LintKind::Eggcorn
        | LintKind::Grammar
        | LintKind::Malapropism
        | LintKind::Nonstandard
        | LintKind::Repetition
        | LintKind::Usage
        | LintKind::WordOrder => UnderlineKind::Red,

        // Style and clarity.
        LintKind::Enhancement
        | LintKind::Formatting
        | LintKind::Readability
        | LintKind::Redundancy
        | LintKind::Regionalism
        | LintKind::Style
        | LintKind::WordChoice => UnderlineKind::Blue,

        // Advisory.
        LintKind::Miscellaneous => UnderlineKind::Gray,
    }
}

/// The colour a lint kind is drawn in.
pub fn lint_kind_color(lint_kind: LintKind) -> Color {
    underline_kind(lint_kind).color()
}

pub fn lint_kind_color_from_key(lint_kind_key: &str) -> Option<Color> {
    LintKind::from_string_key(lint_kind_key).map(lint_kind_color)
}

pub fn lint_kind_text_color(lint_kind: LintKind) -> TextColor {
    contrasting_text_color(lint_kind_color(lint_kind))
}

pub fn contrasting_text_color(color: Color) -> TextColor {
    let luminance =
        0.299 * f64::from(color.r) + 0.587 * f64::from(color.g) + 0.114 * f64::from(color.b);

    if luminance > 186.0 {
        TextColor::Black
    } else {
        TextColor::White
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lint_kind_has_an_underline_colour() {
        for kind in LINT_KINDS {
            let color = lint_kind_color(kind);
            // The colour must not be black, or the underline would be
            // invisible on a light document.
            assert!(
                color.r != 0 || color.g != 0 || color.b != 0,
                "{kind:?} has an invisible colour"
            );
        }
    }

    #[test]
    fn correctness_is_red() {
        assert_eq!(underline_kind(LintKind::Spelling), UnderlineKind::Red);
        assert_eq!(underline_kind(LintKind::Grammar), UnderlineKind::Red);
        assert_eq!(underline_kind(LintKind::Punctuation), UnderlineKind::Red);
        assert_eq!(underline_kind(LintKind::Capitalization), UnderlineKind::Red);
    }

    #[test]
    fn style_is_blue() {
        assert_eq!(underline_kind(LintKind::WordChoice), UnderlineKind::Blue);
        assert_eq!(underline_kind(LintKind::Redundancy), UnderlineKind::Blue);
        assert_eq!(underline_kind(LintKind::Enhancement), UnderlineKind::Blue);
    }

    #[test]
    fn the_three_colours_are_distinct() {
        assert_ne!(UnderlineKind::Red.color(), UnderlineKind::Blue.color());
        assert_ne!(UnderlineKind::Blue.color(), UnderlineKind::Gray.color());
        assert_ne!(UnderlineKind::Red.color(), UnderlineKind::Gray.color());
    }

    #[test]
    fn the_red_is_actually_red() {
        let red = UnderlineKind::Red.color();
        assert!(red.r > red.b, "the correctness colour should read as red");
    }

    #[test]
    fn the_blue_is_actually_blue() {
        let blue = UnderlineKind::Blue.color();
        assert!(blue.b > blue.r, "the style colour should read as blue");
    }

    #[test]
    fn the_grey_is_neutral() {
        let gray = UnderlineKind::Gray.color();
        let spread = (gray.r as i32 - gray.g as i32).abs() + (gray.g as i32 - gray.b as i32).abs();
        assert!(spread < 30, "the advisory colour should be neutral");
    }
}
