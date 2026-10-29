use accessibility::attribute::AXUIElementAttributes;
use accessibility::ui_element::AXUIElement;
use accessibility::{Error, TreeVisitor, TreeWalker, TreeWalkerFlow};
use accessibility_sys::{
    AXUIElementCopyAttributeValue, AXUIElementCopyParameterizedAttributeValue, AXUIElementGetPid,
    AXUIElementSetAttributeValue, AXValueCreate, AXValueGetType, AXValueGetValue, AXValueRef,
    kAXBoundsForRangeParameterizedAttribute, kAXErrorIllegalArgument, kAXErrorNoValue,
    kAXErrorParameterizedAttributeUnsupported, kAXErrorSuccess, kAXPositionAttribute,
    kAXSelectedTextRangeAttribute, kAXSizeAttribute, kAXValueTypeCFRange, kAXValueTypeCGPoint,
    kAXValueTypeCGRect, kAXValueTypeCGSize, pid_t,
};
use core::{ffi::c_void, mem::MaybeUninit};
use core_foundation::base::{CFRange, CFType, TCFType};
use core_foundation::string::CFString;
use harper_core::{
    Span,
    linting::{Lint, LintKind, Suggestion},
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::{
    cell::{Cell, RefCell},
    ptr,
    time::{Duration, Instant},
};

use crate::llm_cache::{self, CachedAnalysis, CachedSuggestion};
use crate::rect::{ActionableLint, Rect};

use super::LintCallback;

/// Safety valve for very large or unresponsive accessibility trees (Mail, web views).
const MAX_TREE_NODES: usize = 20_000;
const MAX_TREE_WALK_DURATION: Duration = Duration::from_millis(1_000);

/// Outcome of asking macOS for the bounds of a text range.
pub enum TextRangeBoundsProbe {
    Success(Rect),
    Unavailable,
    InvalidRangeValue,
    AxError(i32),
}

impl TextRangeBoundsProbe {
    /// Returns true only when the probe produced non-zero text geometry.
    pub fn has_usable_text_metrics(self) -> bool {
        matches!(self, Self::Success(_))
    }
}

/// Converts a Core Foundation value to a Rust string when it is a `CFString`.
pub fn cf_type_to_string(value: &CFType) -> Option<String> {
    value
        .instance_of::<CFString>()
        .then(|| unsafe { CFString::wrap_under_get_rule(value.as_CFTypeRef() as _) }.to_string())
}

/// Probes `AXBoundsForRange` for a specific text range on an element.
pub fn probe_element_rect_for_text_range(
    element: &AXUIElement,
    start_index: isize,
    length: isize,
) -> TextRangeBoundsProbe {
    let range = CFRange {
        location: start_index,
        length,
    };

    let range_value_ref = unsafe {
        AXValueCreate(
            kAXValueTypeCFRange,
            &range as *const CFRange as *const c_void,
        )
    };

    if range_value_ref.is_null() {
        return TextRangeBoundsProbe::InvalidRangeValue;
    }

    let range_value = unsafe { CFType::wrap_under_create_rule(range_value_ref as _) };
    let attr = CFString::new(kAXBoundsForRangeParameterizedAttribute);
    let mut value = ptr::null();

    let error = unsafe {
        AXUIElementCopyParameterizedAttributeValue(
            element.as_concrete_TypeRef(),
            attr.as_concrete_TypeRef(),
            range_value.as_CFTypeRef(),
            &mut value,
        )
    };

    if error == kAXErrorNoValue || error == kAXErrorParameterizedAttributeUnsupported {
        return TextRangeBoundsProbe::Unavailable;
    }

    if error != kAXErrorSuccess {
        return TextRangeBoundsProbe::AxError(error);
    }

    if value.is_null() {
        return TextRangeBoundsProbe::Unavailable;
    }

    let value = unsafe { CFType::wrap_under_create_rule(value) };
    let ax_value = value.as_CFTypeRef() as AXValueRef;
    let value_type = unsafe { AXValueGetType(ax_value) };

    if value_type != kAXValueTypeCGRect {
        return TextRangeBoundsProbe::Unavailable;
    }

    let mut rect = MaybeUninit::<NSRect>::uninit();

    let ok = unsafe {
        AXValueGetValue(
            ax_value,
            kAXValueTypeCGRect,
            rect.as_mut_ptr() as *mut c_void,
        )
    };

    if !ok {
        return TextRangeBoundsProbe::Unavailable;
    }

    let rect = unsafe { rect.assume_init() };

    let rect = Rect {
        x: rect.origin.x,
        y: rect.origin.y,
        width: rect.size.width,
        height: rect.size.height,
    };

    if rect_has_usable_text_metrics(rect) {
        TextRangeBoundsProbe::Success(rect)
    } else {
        TextRangeBoundsProbe::Unavailable
    }
}

/// Returns whether a text rectangle has enough geometry to render a highlight.
fn rect_has_usable_text_metrics(rect: Rect) -> bool {
    rect.width > 0.0 && rect.height > 0.0
}

/// Reads the screen-space frame (`AXPosition` and `AXSize`) of an accessibility element.
pub fn element_frame(element: &AXUIElement) -> Option<Rect> {
    let pos_attr = CFString::new(kAXPositionAttribute);
    let mut pos_ref = ptr::null();
    let pos_err = unsafe {
        AXUIElementCopyAttributeValue(
            element.as_concrete_TypeRef(),
            pos_attr.as_concrete_TypeRef(),
            &mut pos_ref,
        )
    };
    if pos_err != kAXErrorSuccess || pos_ref.is_null() {
        return None;
    }
    let pos_val = unsafe { CFType::wrap_under_create_rule(pos_ref) };
    let pos_ax = pos_val.as_CFTypeRef() as AXValueRef;
    if unsafe { AXValueGetType(pos_ax) } != kAXValueTypeCGPoint {
        return None;
    }
    let mut point = MaybeUninit::<NSPoint>::uninit();
    let pos_ok = unsafe {
        AXValueGetValue(
            pos_ax,
            kAXValueTypeCGPoint,
            point.as_mut_ptr() as *mut c_void,
        )
    };
    if !pos_ok {
        return None;
    }
    let point = unsafe { point.assume_init() };

    let size_attr = CFString::new(kAXSizeAttribute);
    let mut size_ref = ptr::null();
    let size_err = unsafe {
        AXUIElementCopyAttributeValue(
            element.as_concrete_TypeRef(),
            size_attr.as_concrete_TypeRef(),
            &mut size_ref,
        )
    };
    if size_err != kAXErrorSuccess || size_ref.is_null() {
        return None;
    }
    let size_val = unsafe { CFType::wrap_under_create_rule(size_ref) };
    let size_ax = size_val.as_CFTypeRef() as AXValueRef;
    if unsafe { AXValueGetType(size_ax) } != kAXValueTypeCGSize {
        return None;
    }
    let mut size = MaybeUninit::<NSSize>::uninit();
    let size_ok = unsafe {
        AXValueGetValue(
            size_ax,
            kAXValueTypeCGSize,
            size.as_mut_ptr() as *mut c_void,
        )
    };
    if !size_ok {
        return None;
    }
    let size = unsafe { size.assume_init() };

    let rect = Rect {
        x: point.x,
        y: point.y,
        width: size.width,
        height: size.height,
    };
    rect_has_usable_text_metrics(rect).then_some(rect)
}

/// Computes the visible screen-space viewport of `element` by intersecting its frame
/// with all enclosing ancestor frames (`AXScrollArea`, `AXClipView`, `AXWindow`, etc.).
/// This prevents text ranges that have scrolled out of view or overflowed horizontally
/// from bleeding outside the visible text container.
pub fn visible_element_frame(element: &AXUIElement) -> Option<Rect> {
    let mut visible = element_frame(element)?;
    let mut current = element.clone();
    for _ in 0..10 {
        let Ok(parent) =
            super::core_foundation_utilities::ax_element_attribute(&current, "AXParent")
        else {
            break;
        };
        if let Ok(role) = parent.role() {
            let role_str = role.to_string();
            if role_str == "AXApplication" {
                break;
            }
            if matches!(
                role_str.as_str(),
                "AXScrollArea" | "AXClipView" | "AXWindow"
            ) && let Some(parent_rect) = element_frame(&parent)
                && let Some(clipped) = visible.intersection(parent_rect)
                && rect_has_usable_text_metrics(clipped)
            {
                visible = clipped;
            }
        }
        current = parent;
    }
    rect_has_usable_text_metrics(visible).then_some(visible)
}

/// Estimates a character range's bounds inside an element frame when `AXBoundsForRange` is unavailable.
fn estimate_rect_within_frame(
    frame: Rect,
    full_text: &str,
    start_index: usize,
    length: usize,
) -> Option<Rect> {
    if !rect_has_usable_text_metrics(frame) || length == 0 {
        return None;
    }
    let padding_x = 8.0_f64;
    let padding_y = 6.0_f64;
    let char_width = 7.6_f64;
    let line_height = 18.0_f64;
    let usable_width = (frame.width - 2.0 * padding_x).max(40.0);
    let chars_per_line = ((usable_width / char_width).floor() as usize).max(10);

    let mut line = 0usize;
    let mut col = 0usize;
    for (idx, ch) in full_text.chars().enumerate() {
        if idx >= start_index {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
            if col >= chars_per_line {
                line += 1;
                col = 0;
            }
        }
    }

    let x = (frame.x + padding_x + (col as f64) * char_width)
        .min((frame.x + frame.width - 16.0).max(frame.x));
    let y = (frame.y + padding_y + (line as f64) * line_height)
        .min((frame.y + frame.height - line_height).max(frame.y));
    let width = ((length as f64) * char_width).clamp(12.0, (frame.x + frame.width - x).max(12.0));
    let height = line_height.min(frame.height.max(12.0));

    Some(Rect {
        x,
        y,
        width,
        height,
    })
}

/// Collects lint rectangles while walking supported text elements in an AX tree.
/// The text the user has selected in the active field.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSelection {
    /// Char offset of the selection in the field's text.
    pub char_start: usize,
    /// Char offset (exclusive) of the selection in the field's text.
    pub char_end: usize,
    /// The selected text.
    pub text: String,
    /// Screen bounds of the selection, clipped to the visible field.
    pub rect: Rect,
}

pub struct RectCollector<'a> {
    rects: RefCell<Vec<ActionableLint>>,
    expected_rect_count: Cell<usize>,
    read_failed: Cell<bool>,
    found_text_element: Cell<bool>,
    field_rect: RefCell<Option<Rect>>,
    visited: Cell<usize>,
    started: Instant,
    pid: i32,
    cached: Option<CachedAnalysis>,
    lint_text: RefCell<&'a mut LintCallback<'a>>,
    /// The tone detected for the field, set while the tree is walked.
    ///
    /// Detection is local and deterministic, so the badge on the widget costs
    /// nothing and works offline.
    tone: RefCell<Option<String>>,
    /// Text of the first supported editable text element visited.
    source_text: RefCell<Option<String>>,
    /// The non-empty selection in that element, with its on-screen bounds.
    selection: RefCell<Option<FieldSelection>>,
}

impl TreeVisitor for RectCollector<'_> {
    fn enter_element(&self, element: &AXUIElement) -> TreeWalkerFlow {
        let visited = self.visited.get().saturating_add(1);
        self.visited.set(visited);
        if visited > MAX_TREE_NODES || self.started.elapsed() > MAX_TREE_WALK_DURATION {
            return TreeWalkerFlow::Exit;
        }

        if !is_supported_text_element(element) {
            return TreeWalkerFlow::Continue;
        }

        if super::core_foundation_utilities::ax_string_attribute(element, "AXSubrole")
            .is_ok_and(|subrole| subrole == "AXSecureTextField")
        {
            return TreeWalkerFlow::Continue;
        }

        self.found_text_element.set(true);
        let element_visible_rect =
            visible_element_frame(element).or_else(|| element_frame(element));
        if self.field_rect.borrow().is_none() {
            *self.field_rect.borrow_mut() = element_visible_rect;
        }

        let string = match read_element_text(element) {
            Some(s) if !s.is_empty() => s,
            _ => return TreeWalkerFlow::Exit,
        };

        if self.source_text.borrow().is_none() {
            *self.source_text.borrow_mut() = Some(string.clone());
            *self.selection.borrow_mut() =
                read_field_selection(element, &string, element_visible_rect);
        }

        if !string.trim().is_empty() && self.tone.borrow().is_none() {
            let signal = elocutio_analysis::detect_tone(&string);
            *self.tone.borrow_mut() = Some(signal.tone.label().to_string());
        }

        let mut rects = self.rects.borrow_mut();

        // 1. When a fresh AI provider analysis exists for this exact field and text hash,
        // prioritize the AI's context-aware suggestions first so they always supersede
        // context-blind dictionary/rule guesses.
        let has_fresh_llm_cache = self
            .cached
            .as_ref()
            .is_some_and(|c| c.pid == self.pid && c.text_hash == llm_cache::text_hash(&string));

        if let Some(cached) = &self.cached
            && has_fresh_llm_cache
        {
            for suggestion in &cached.suggestions {
                if suggestion.char_end > string.chars().count()
                    || suggestion.char_start >= suggestion.char_end
                    || rects.iter().any(|existing| {
                        existing.lint.span.start < suggestion.char_end
                            && suggestion.char_start < existing.lint.span.end
                    })
                {
                    continue;
                }

                self.expected_rect_count
                    .set(self.expected_rect_count.get() + 1);

                let Some(raw_rect) = element_rect_for_text_range_with_fallback(
                    element,
                    &string,
                    suggestion.char_start,
                    suggestion.char_end.saturating_sub(suggestion.char_start),
                ) else {
                    continue;
                };

                let Some(rect) = (match element_visible_rect {
                    Some(vf) => raw_rect.intersection(vf),
                    None => Some(raw_rect),
                }) else {
                    continue;
                };
                if rect.width < 6.0 || rect.height < 8.0 {
                    continue;
                }

                let lint = cached_lint(suggestion);
                let element = element.clone();
                let source_text = string.clone();
                let suggestion_source_text = string.clone();
                let suggestion_lint = lint.clone();
                let rule_name = format!("llm:{}", category_name(suggestion));
                let suggestion_rule_name = rule_name.clone();

                rects.push(ActionableLint::new(
                    rect,
                    rule_name,
                    lint,
                    source_text,
                    move |suggestion| {
                        apply_suggestion_to_element(
                            element,
                            suggestion_source_text,
                            suggestion_lint,
                            suggestion,
                            &suggestion_rule_name,
                        );
                    },
                ));
            }
        }

        // 2. Run local Harper lints:
        //    - When `has_fresh_llm_cache` is true, skip Harper's context-blind `SpellCheck`
        //      and any lint that overlaps an AI suggestion, keeping only deterministic
        //      non-overlapping rules (e.g. `RepeatedWords`, `AnA`).
        //    - When offline or before the AI response arrives, cap `SpellCheck` suggestions
        //      to at most 3 candidates so Harper never dumps long out-of-context word lists.
        let organized_lints = (self.lint_text.borrow_mut())(&string);

        for (rule_name, lints) in organized_lints {
            if has_fresh_llm_cache && rule_name == "SpellCheck" {
                continue;
            }
            for mut lint in lints {
                if rects.iter().any(|existing| {
                    existing.lint.span.start < lint.span.end
                        && lint.span.start < existing.lint.span.end
                }) {
                    continue;
                }
                if rule_name == "SpellCheck" && lint.suggestions.len() > 3 {
                    lint.suggestions.truncate(3);
                }

                self.expected_rect_count
                    .set(self.expected_rect_count.get() + 1);

                if let Some(raw_rect) = element_rect_for_text_range_with_fallback(
                    element,
                    &string,
                    lint.span.start,
                    lint.span.len(),
                ) {
                    let Some(rect) = (match element_visible_rect {
                        Some(vf) => raw_rect.intersection(vf),
                        None => Some(raw_rect),
                    }) else {
                        continue;
                    };
                    if rect.width < 6.0 || rect.height < 8.0 {
                        continue;
                    }
                    let element = element.clone();
                    let source_text = string.clone();
                    let suggestion_source_text = string.clone();
                    let suggestion_lint = lint.clone();
                    let suggestion_rule_name = rule_name.clone();

                    rects.push(ActionableLint::new(
                        rect,
                        rule_name.clone(),
                        lint,
                        source_text,
                        move |suggestion| {
                            apply_suggestion_to_element(
                                element,
                                suggestion_source_text,
                                suggestion_lint,
                                suggestion,
                                &suggestion_rule_name,
                            );
                        },
                    ));
                }
            }
        }

        TreeWalkerFlow::Exit
    }
    fn exit_element(&self, _element: &AXUIElement) {}
}

impl<'a> RectCollector<'a> {
    pub fn new(lint_text: &'a mut LintCallback, cached: Option<CachedAnalysis>, pid: i32) -> Self {
        Self {
            rects: RefCell::new(Vec::new()),
            expected_rect_count: Cell::new(0),
            read_failed: Cell::new(false),
            found_text_element: Cell::new(false),
            field_rect: RefCell::new(None),
            visited: Cell::new(0),
            started: Instant::now(),
            pid,
            cached,
            lint_text: RefCell::new(lint_text),
            tone: RefCell::new(None),
            source_text: RefCell::new(None),
            selection: RefCell::new(None),
        }
    }

    /// The non-empty selection in the active field, if any.
    pub fn selection(&self) -> Option<FieldSelection> {
        self.selection.borrow().clone()
    }

    /// Whether at least one supported editable text element was visited.
    pub fn found_text_element(&self) -> bool {
        self.found_text_element.get()
    }

    /// Screen bounds of the first supported editable text element visited.
    pub fn field_rect(&self) -> Option<Rect> {
        *self.field_rect.borrow()
    }

    /// The tone detected for the field, if it was clear enough to name.
    pub fn detected_tone(&self) -> Option<String> {
        self.tone.borrow().clone()
    }

    /// The text of the active field, even when it has zero lints.
    pub fn source_text(&self) -> Option<String> {
        self.source_text.borrow().clone()
    }

    pub fn unwrap_rects(self) -> Option<Vec<ActionableLint>> {
        let mut rects = self.rects.into_inner();

        if rects.is_empty() && (self.expected_rect_count.get() > 0 || self.read_failed.get()) {
            return None;
        }

        rects.sort_by_key(|actionable| {
            (
                actionable.rect.y as i32,
                actionable.rect.x as i32,
                actionable.lint.span.start,
                actionable.lint.priority,
            )
        });
        Some(rects)
    }
}

fn cached_lint(suggestion: &CachedSuggestion) -> Lint {
    let lint_kind = match suggestion.category {
        elocutio_core::Category::Grammar => LintKind::Grammar,
        elocutio_core::Category::Spelling => LintKind::Spelling,
        elocutio_core::Category::Punctuation => LintKind::Punctuation,
        elocutio_core::Category::Capitalization => LintKind::Capitalization,
        elocutio_core::Category::Clarity => LintKind::Enhancement,
        elocutio_core::Category::Conciseness => LintKind::Redundancy,
        elocutio_core::Category::WordChoice => LintKind::WordChoice,
        elocutio_core::Category::Consistency => LintKind::Style,
        elocutio_core::Category::Tone => LintKind::Style,
        elocutio_core::Category::Style => LintKind::Style,
        elocutio_core::Category::Formatting => LintKind::Formatting,
    };

    let replacement = if suggestion.replacement.is_empty() {
        Suggestion::Remove
    } else {
        Suggestion::ReplaceWith(suggestion.replacement.chars().collect())
    };

    Lint {
        span: Span::new(suggestion.char_start, suggestion.char_end),
        lint_kind,
        suggestions: vec![replacement],
        message: suggestion.explanation.clone(),
        priority: severity_priority(&suggestion.severity),
    }
}

fn severity_priority(severity: &elocutio_core::Severity) -> u8 {
    match severity {
        elocutio_core::Severity::High => 5,
        elocutio_core::Severity::Medium => 20,
        elocutio_core::Severity::Low => 40,
    }
}

fn category_name(suggestion: &CachedSuggestion) -> &'static str {
    match suggestion.category {
        elocutio_core::Category::Grammar => "grammar",
        elocutio_core::Category::Spelling => "spelling",
        elocutio_core::Category::Punctuation => "punctuation",
        elocutio_core::Category::Capitalization => "capitalization",
        elocutio_core::Category::Clarity => "clarity",
        elocutio_core::Category::Conciseness => "conciseness",
        elocutio_core::Category::WordChoice => "word_choice",
        elocutio_core::Category::Consistency => "consistency",
        elocutio_core::Category::Tone => "tone",
        elocutio_core::Category::Style => "style",
        elocutio_core::Category::Formatting => "formatting",
    }
}

/// Reads the full text of a supported accessibility text element, falling back to
/// `AXStaticText` descendants when `AXValue` is empty or unavailable (e.g. rich `contenteditable` views).
pub fn read_element_text(element: &AXUIElement) -> Option<String> {
    let direct = element.value().ok().and_then(|v| cf_type_to_string(&v));
    if let Some(ref s) = direct
        && !s.is_empty()
    {
        return direct;
    }
    let walker = TreeWalker::new();
    let collector = StaticTextCollector::new();
    walker.walk(element, &collector);
    let parts: Vec<String> = collector
        .into_texts()
        .into_iter()
        .map(|(_, text)| text)
        .collect();
    if !parts.is_empty() {
        Some(parts.join("\n"))
    } else {
        direct
    }
}

/// Reads the element's own `AXValue` string, ignoring any `AXStaticText` descendants.
///
/// Writes must be built from this, never from [`read_element_text`]'s joined fallback: writing a
/// joined string back would flatten the field's structure.
fn read_ax_value(element: &AXUIElement) -> Option<String> {
    element
        .value()
        .ok()
        .and_then(|value| cf_type_to_string(&value))
        .filter(|text| !text.is_empty())
}

/// The `(location, length)` in UTF-16 units of the char range `start..end` of `chars`.
///
/// Spans are counted in chars, but Accessibility ranges are UTF-16, so any text with characters
/// outside the Basic Multilingual Plane (emoji, for one) needs this conversion.
fn utf16_range(chars: &[char], start: usize, end: usize) -> (isize, isize) {
    let start = start.min(chars.len());
    let end = end.min(chars.len()).max(start);
    let utf16_len =
        |chars: &[char]| -> isize { chars.iter().map(|c| c.len_utf16() as isize).sum() };
    (utf16_len(&chars[..start]), utf16_len(&chars[start..end]))
}

/// The char index of UTF-16 offset `utf16` in `chars`, rounding a split surrogate pair forward.
fn char_index_for_utf16(chars: &[char], utf16: usize) -> usize {
    let mut units = 0usize;
    for (index, c) in chars.iter().enumerate() {
        if units >= utf16 {
            return index;
        }
        units += c.len_utf16();
    }
    chars.len()
}

/// Reads the field's selection and where it sits on screen.
///
/// Returns `None` for a collapsed caret, a range the field cannot place on screen, or a
/// selection entirely outside the visible part of the field.
fn read_field_selection(
    element: &AXUIElement,
    text: &str,
    visible_rect: Option<Rect>,
) -> Option<FieldSelection> {
    let (location, length) = get_selected_utf16_range(element)?;
    if location < 0 || length <= 0 {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let char_start = char_index_for_utf16(&chars, location as usize);
    let char_end = char_index_for_utf16(&chars, (location + length) as usize).max(char_start);
    let selected: String = chars[char_start..char_end].iter().collect();
    if selected.trim().is_empty() {
        return None;
    }
    let raw_rect = element_rect_for_text_range_with_fallback(
        element,
        text,
        char_start,
        char_end - char_start,
    )?;
    let rect = match visible_rect {
        Some(visible) => raw_rect.intersection(visible)?,
        None => raw_rect,
    };
    Some(FieldSelection {
        char_start,
        char_end,
        text: selected,
        rect,
    })
}

/// Finds where a span checked against `source` now sits in the field's `live` text.
///
/// Polling freezes while the suggestion card is open, so the user may have typed since the text
/// was checked. The span is accepted where it was if the same text is still there, otherwise at
/// the one nearby occurrence of that text. Either way it must sit at the same kind of word
/// boundary as before, so a word the user has since extended is not "found" inside a longer word.
/// `None` means the span cannot be placed safely and nothing should be written.
fn relocate_span(
    source: &[char],
    live: &[char],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    if start > end || end > source.len() {
        return None;
    }
    if source == live {
        return Some((start, end));
    }

    let original = &source[start..end];
    if original.is_empty() {
        return None;
    }
    let len = original.len();
    let is_word_char = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric());
    let fits_at = |position: usize| {
        live.get(position..position + len) == Some(original)
            && is_word_char(position.checked_sub(1).and_then(|i| live.get(i)))
                == is_word_char(start.checked_sub(1).and_then(|i| source.get(i)))
            && is_word_char(live.get(position + len)) == is_word_char(source.get(end))
    };

    if fits_at(start) {
        return Some((start, end));
    }

    // Typing before the span shifts it by about the change in length.
    let window = live.len().abs_diff(source.len()) + 32;
    let first = start.saturating_sub(window);
    let last = (start + window).min(live.len().saturating_sub(len));
    let mut candidates = (first..=last).filter(|&position| fits_at(position));
    let position = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    Some((position, position + len))
}

pub(super) fn get_selected_utf16_range(element: &AXUIElement) -> Option<(isize, isize)> {
    let attr = CFString::new(kAXSelectedTextRangeAttribute);
    let mut value = ptr::null();
    let error = unsafe {
        AXUIElementCopyAttributeValue(
            element.as_concrete_TypeRef(),
            attr.as_concrete_TypeRef(),
            &mut value,
        )
    };
    if error != kAXErrorSuccess || value.is_null() {
        return None;
    }
    let value = unsafe { CFType::wrap_under_create_rule(value) };
    let ax_value = value.as_CFTypeRef() as AXValueRef;
    if unsafe { AXValueGetType(ax_value) } != kAXValueTypeCFRange {
        return None;
    }
    let mut range = MaybeUninit::<CFRange>::uninit();
    let ok = unsafe {
        AXValueGetValue(
            ax_value,
            kAXValueTypeCFRange,
            range.as_mut_ptr() as *mut c_void,
        )
    };
    if !ok {
        return None;
    }
    let range = unsafe { range.assume_init() };
    Some((range.location, range.length))
}

fn set_selected_utf16_range(element: &AXUIElement, start: isize, length: isize) -> bool {
    let range = CFRange {
        location: start,
        length,
    };
    let range_value_ref = unsafe {
        AXValueCreate(
            kAXValueTypeCFRange,
            &range as *const CFRange as *const c_void,
        )
    };
    if range_value_ref.is_null() {
        return false;
    }
    let range_value = unsafe { CFType::wrap_under_create_rule(range_value_ref as _) };
    let attr = CFString::new(kAXSelectedTextRangeAttribute);
    let error = unsafe {
        AXUIElementSetAttributeValue(
            element.as_concrete_TypeRef(),
            attr.as_concrete_TypeRef(),
            range_value.as_CFTypeRef(),
        )
    };
    error == kAXErrorSuccess
}

fn set_selected_text_attribute(element: &AXUIElement, replacement: &str) -> bool {
    let attr = CFString::new("AXSelectedText");
    let val = CFString::new(replacement);
    let error = unsafe {
        AXUIElementSetAttributeValue(
            element.as_concrete_TypeRef(),
            attr.as_concrete_TypeRef(),
            val.as_CFTypeRef(),
        )
    };
    error == kAXErrorSuccess
}

fn activate_target_app(pid: pid_t) {
    if pid <= 0 {
        return;
    }
    if let Some(app) =
        objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
    {
        #[allow(deprecated)]
        let _ = app.activateWithOptions(
            objc2_app_kit::NSApplicationActivationOptions::ActivateIgnoringOtherApps,
        );
    }
}

/// What the field looked like after an edit was attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditOutcome {
    /// The field now holds exactly the expected text.
    Expected,
    /// The field changed, but not into the expected text (or could no longer be read).
    ///
    /// The edit is treated as having landed, so it is never applied a second time.
    Changed,
    /// The field still holds the text from before the edit.
    Unchanged,
}

/// Polls the field after an edit. Chromium / Electron / WebKit apply AX writes asynchronously
/// across IPC (~15-80ms), so an in-flight write must not be mistaken for a failure and applied
/// again by a fallback.
fn wait_for_edit(
    element: &AXUIElement,
    before: &str,
    expected: &str,
    read_value_only: bool,
) -> EditOutcome {
    for attempt in 0..8 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(22));
        }
        let after = if read_value_only {
            read_ax_value(element)
        } else {
            read_element_text(element)
        };
        match after {
            Some(after) if after == expected => return EditOutcome::Expected,
            Some(after) if after != before => return EditOutcome::Changed,
            None if attempt >= 3 => return EditOutcome::Changed,
            _ => {}
        }
    }
    EditOutcome::Unchanged
}

/// Selects `utf16_start..+utf16_len` and checks that the selection really holds `original`.
///
/// Replacing or pasting over a selection that does not hold the span would insert the
/// replacement at the wrong place.
fn select_span(
    element: &AXUIElement,
    utf16_start: isize,
    utf16_len: isize,
    original: &str,
) -> bool {
    if !set_selected_utf16_range(element, utf16_start, utf16_len) {
        return false;
    }
    (0..4).any(|attempt| {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(18));
        }
        super::core_foundation_utilities::ax_string_attribute(element, "AXSelectedText")
            .is_ok_and(|selected| selected == original)
    })
}

fn apply_suggestion_to_element(
    element: AXUIElement,
    source_text: String,
    lint: Lint,
    suggestion: Suggestion,
    rule_name: &str,
) {
    let source_chars = source_text.chars().collect::<Vec<_>>();
    let span_start = lint.span.start.min(source_chars.len());
    let span_end = lint.span.end.min(source_chars.len()).max(span_start);
    let original: String = source_chars[span_start..span_end].iter().collect();
    let voice_replacement = match &suggestion {
        Suggestion::ReplaceWith(chars) => Some(chars.iter().collect::<String>()),
        Suggestion::Remove => Some(String::new()),
        Suggestion::InsertAfter(_) => None,
    };
    let text = match &suggestion {
        Suggestion::ReplaceWith(chars) => chars.iter().collect::<String>(),
        Suggestion::Remove => String::new(),
        Suggestion::InsertAfter(chars) => {
            format!("{original}{}", chars.iter().collect::<String>())
        }
    };
    apply_span_edit(
        &element,
        &source_text,
        SpanEdit {
            start: span_start,
            end: span_end,
            text,
            voice_replacement,
            category: format!("{:?}", lint.lint_kind),
            rule_name,
        },
    );
}

/// One replacement of a char span in a field, and what to record once it lands.
pub struct SpanEdit<'a> {
    /// Char offset in the text the edit was computed against.
    pub start: usize,
    /// Char offset (exclusive) in the text the edit was computed against.
    pub end: usize,
    /// The text that replaces the span.
    pub text: String,
    /// The replacement to teach the voice profile; `None` when the edit is not a plain
    /// replacement of the span (an insertion after it, a generative rewrite).
    pub voice_replacement: Option<String>,
    /// The category recorded with the accepted edit.
    pub category: String,
    /// The rule recorded in the suggestion event log.
    pub rule_name: &'a str,
}

/// Replaces chars `edit.start..edit.end` of `source_text` in the field, touching nothing else.
///
/// Used for single lint fixes, whole-sentence corrections, and rewrites of a selection. Returns
/// whether the edit landed; when the field changed so the span can no longer be placed safely,
/// nothing is written and the overlay is asked to re-read the field.
pub fn apply_span_edit(element: &AXUIElement, source_text: &str, edit: SpanEdit) -> bool {
    let source_chars = source_text.chars().collect::<Vec<_>>();
    let span_start = edit.start.min(source_chars.len());
    let span_end = edit.end.min(source_chars.len()).max(span_start);
    let original: String = source_chars[span_start..span_end].iter().collect();
    let span_paste_text = edit.text;

    let mut pid: pid_t = 0;
    let pid_error = unsafe { AXUIElementGetPid(element.as_concrete_TypeRef(), &mut pid) };

    // 1. Re-read the field. Polling freezes while the suggestion card is open, so `source_text`
    // can be stale; writing an edit built from it would erase whatever was typed since.
    let direct_value = read_ax_value(element);
    let value_readable = direct_value.is_some();
    let Some(live_text) = direct_value.or_else(|| read_element_text(element)) else {
        eprintln!("Could not read the field before applying the edit; nothing was changed");
        crate::elo_overlay::request_refresh();
        return false;
    };
    let live_chars = live_text.chars().collect::<Vec<_>>();
    let Some((start, end)) = relocate_span(&source_chars, &live_chars, span_start, span_end) else {
        eprintln!(
            "The field changed since it was checked and the edit no longer lines up; \
             nothing was changed"
        );
        crate::elo_overlay::request_refresh();
        return false;
    };

    let mut updated: String = live_chars[..start].iter().collect();
    updated.push_str(&span_paste_text);
    updated.extend(&live_chars[end..]);
    let (utf16_start, utf16_len) = utf16_range(&live_chars, start, end);
    let replacement_utf16_len = span_paste_text.encode_utf16().count() as isize;
    let total_updated_utf16_len = updated.encode_utf16().count() as isize;

    let prev_selection = get_selected_utf16_range(element);
    let target_caret = match prev_selection {
        Some((loc, 0)) if loc >= utf16_start + utf16_len => {
            (loc - utf16_len + replacement_utf16_len).clamp(0, total_updated_utf16_len)
        }
        Some((loc, 0)) if loc > 0 && loc <= utf16_start => loc.clamp(0, total_updated_utf16_len),
        _ => utf16_start + replacement_utf16_len,
    };

    eprintln!(
        "Applying edit via AX (span {start}:{}, replacement {} chars)",
        end - start,
        span_paste_text.chars().count()
    );

    activate_target_app(pid);
    let mut applied = false;

    // 2. Preferred: select exactly the span and replace the selection. This touches nothing
    // outside the span, so it cannot lose concurrent edits elsewhere in the field.
    if select_span(element, utf16_start, utf16_len, &original)
        && set_selected_text_attribute(element, &span_paste_text)
    {
        applied =
            wait_for_edit(element, &live_text, &updated, value_readable) != EditOutcome::Unchanged;
    }

    // 3. Fallback: write the whole value, built from the live `AXValue` read above, and only
    // while the field still holds exactly that. A field whose text only exists as static-text
    // children is never rewritten wholesale.
    if !applied
        && value_readable
        && read_ax_value(element).as_deref() == Some(live_text.as_str())
        && element
            .set_value(CFString::new(&updated).as_CFType())
            .is_ok()
    {
        applied = wait_for_edit(element, &live_text, &updated, true) != EditOutcome::Unchanged;
    }

    // 4. Last resort: paste over the selected span, once the target app is frontmost.
    if !applied
        && select_span(element, utf16_start, utf16_len, &original)
        && super::activate_and_wait_until_frontmost(pid)
        && let Ok(snapshot) = crate::pasteboard::write_transient(&span_paste_text)
    {
        if super::paste_command_v().is_ok() {
            std::thread::sleep(Duration::from_millis(120));
            applied = wait_for_edit(element, &live_text, &updated, value_readable)
                != EditOutcome::Unchanged;
        }
        crate::pasteboard::restore(snapshot);
    }

    if !applied {
        eprintln!("Unable to apply the edit via AX write or pasteboard fallback");
        return false;
    }

    let _ = set_selected_utf16_range(element, target_caret, 0);

    if pid_error == kAXErrorSuccess
        && let Err(error) = crate::undo::record(pid, &live_text, &updated)
    {
        eprintln!("Could not record undo: {error}");
    }
    // Recorded here, once, and only after the edit landed.
    if let Some(replacement) = edit.voice_replacement
        && let Err(error) = crate::voice::record_accepted(&original, &replacement, &edit.category)
    {
        eprintln!("Could not record accepted edit: {error}");
    }
    crate::events::record_suggestion_event(
        crate::events::EventKind::Accepted,
        edit.rule_name,
        original.chars().count(),
        span_paste_text.chars().count(),
    );
    true
}

/// Returns whether an accessibility element has a boolean `AXEditable` attribute set to true.
fn is_ax_editable(el: &AXUIElement) -> bool {
    let attribute = CFString::new("AXEditable");
    let mut value = ptr::null();
    let error = unsafe {
        AXUIElementCopyAttributeValue(
            el.as_concrete_TypeRef(),
            attribute.as_concrete_TypeRef(),
            &mut value,
        )
    };
    if error != kAXErrorSuccess || value.is_null() {
        return false;
    }
    let value = unsafe { CFType::wrap_under_create_rule(value) };
    if !value.instance_of::<core_foundation::boolean::CFBoolean>() {
        return false;
    }
    let b = unsafe {
        core_foundation::boolean::CFBoolean::wrap_under_get_rule(value.as_CFTypeRef() as _)
    };
    bool::from(b)
}

/// Walks up `AXParent` (up to 8 levels) or `AXHighestEditableAncestor` / `AXEditableAncestor`
/// to find an enclosing supported editable text element.
pub fn enclosing_supported_text_element(el: &AXUIElement) -> Option<AXUIElement> {
    if super::core_foundation_utilities::ax_string_attribute(el, "AXSubrole")
        .is_ok_and(|subrole| subrole == "AXSecureTextField")
    {
        return None;
    }
    if is_supported_text_element(el) {
        return Some(el.clone());
    }
    if let Ok(highest) =
        super::core_foundation_utilities::ax_element_attribute(el, "AXHighestEditableAncestor")
        && is_supported_text_element(&highest)
    {
        return Some(highest);
    }
    if let Ok(editable) =
        super::core_foundation_utilities::ax_element_attribute(el, "AXEditableAncestor")
        && is_supported_text_element(&editable)
    {
        return Some(editable);
    }
    let mut current = el.clone();
    for _ in 0..8 {
        let Ok(parent) =
            super::core_foundation_utilities::ax_element_attribute(&current, "AXParent")
        else {
            break;
        };
        if is_supported_text_element(&parent) {
            return Some(parent);
        }
        current = parent;
    }
    None
}

/// Returns whether an accessibility element has a text role Elocutio can lint.
pub fn is_supported_text_element(el: &AXUIElement) -> bool {
    if let Ok(role) = el.role() {
        let role_str = role.to_string();
        if is_supported_text_role(&role_str) {
            return true;
        }
        if matches!(
            role_str.as_str(),
            "AXWebArea" | "AXGroup" | "AXDocument" | "AXLayoutArea"
        ) && is_ax_editable(el)
        {
            return true;
        }
    }

    false
}

/// Returns whether an AX role represents editable text supported by the highlighter.
fn is_supported_text_role(role: &str) -> bool {
    matches!(
        role,
        "AXTextArea" | "AXTextField" | "AXSearchField" | "AXComboBox"
    )
}

/// Collects `AXStaticText` descendants with their string values.
///
/// Chromium-based apps (e.g. Slack) report degenerate `AXBoundsForRange`
/// rects on the editable `AXTextArea` wrapper but expose usable bounds on the
/// `AXStaticText` leaf nodes inside it.
struct StaticTextCollector {
    texts: RefCell<Vec<(AXUIElement, String)>>,
}

impl StaticTextCollector {
    fn new() -> Self {
        Self {
            texts: RefCell::new(Vec::new()),
        }
    }

    fn into_texts(self) -> Vec<(AXUIElement, String)> {
        self.texts.into_inner()
    }
}

impl TreeVisitor for StaticTextCollector {
    fn enter_element(&self, element: &AXUIElement) -> TreeWalkerFlow {
        if let Ok(role) = element.role()
            // Some apps (Mail's compose body, for one) expose their text as a
            // static-text child of the editable element rather than as the
            // element's own value, and the editable element reports no range
            // bounds of its own. The fallback therefore accepts the roles
            // that actually carry rendered text.
            && matches!(&*role.to_string(), "AXStaticText" | "AXText")
            && let Ok(value) = element.value()
            && let Some(string) = cf_type_to_string(&value)
            && !string.is_empty()
        {
            self.texts.borrow_mut().push((element.clone(), string));
        }

        TreeWalkerFlow::Continue
    }

    fn exit_element(&self, _element: &AXUIElement) {}
}

/// Finds `needle` within `haystack` starting at char offset `from`.
fn find_char_subslice(haystack: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }

    let end = haystack.len() - needle.len();
    (from..=end).find(|&i| haystack[i..i + needle.len()] == *needle)
}

/// Resolves text-range bounds, falling back to `AXStaticText` descendants
/// when the element itself reports unusable bounds (Chromium-based apps).
///
/// `start_index` and `length` are in chars; they are converted to the UTF-16 ranges
/// `AXBoundsForRange` expects before asking the element.
pub fn element_rect_for_text_range_with_fallback(
    element: &AXUIElement,
    full_text: &str,
    start_index: usize,
    length: usize,
) -> Option<Rect> {
    let full_chars: Vec<char> = full_text.chars().collect();
    let (utf16_start, utf16_len) = utf16_range(&full_chars, start_index, start_index + length);
    if let Ok(Some(rect)) = element_rect_for_text_range(element, utf16_start, utf16_len) {
        return Some(rect);
    }

    let walker = TreeWalker::new();
    let collector = StaticTextCollector::new();
    walker.walk(element, &collector);

    let mut cursor = 0usize;

    for (child, child_text) in collector.into_texts() {
        let child_chars: Vec<char> = child_text.chars().collect();
        // Locate this child's text within the parent value, starting at the
        // cursor, to absorb any separators the parent inserts between children.
        let Some(child_start) = find_char_subslice(&full_chars, &child_chars, cursor) else {
            continue;
        };
        let child_end = child_start + child_chars.len();

        if start_index >= child_start && start_index + length <= child_end {
            let child_offset = start_index - child_start;
            let (utf16_start, utf16_len) =
                utf16_range(&child_chars, child_offset, child_offset + length);
            if let Ok(Some(rect)) = element_rect_for_text_range(&child, utf16_start, utf16_len) {
                return Some(rect);
            }
        }

        cursor = child_end;
    }

    if let Some(frame) = element_frame(element) {
        return estimate_rect_within_frame(frame, full_text, start_index, length);
    }

    None
}

fn element_rect_for_text_range(
    element: &AXUIElement,
    start_index: isize,
    length: isize,
) -> Result<Option<Rect>, Error> {
    match probe_element_rect_for_text_range(element, start_index, length) {
        TextRangeBoundsProbe::Success(rect) => Ok(Some(rect)),
        TextRangeBoundsProbe::InvalidRangeValue => Err(Error::Ax(kAXErrorIllegalArgument)),
        TextRangeBoundsProbe::AxError(error) => Err(Error::Ax(error)),
        TextRangeBoundsProbe::Unavailable => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rect::Rect;

    #[test]
    fn supported_text_roles_include_text_area_and_text_field() {
        assert!(is_supported_text_role("AXTextArea"));
        assert!(is_supported_text_role("AXTextField"));
    }

    #[test]
    fn supported_text_roles_reject_unrelated_roles() {
        assert!(!is_supported_text_role("AXButton"));
        assert!(!is_supported_text_role("AXStaticText"));
        assert!(!is_supported_text_role(""));
    }

    #[test]
    fn finds_char_subslice_respecting_offset() {
        let haystack: Vec<char> = "one\ntwo two".chars().collect();
        let needle: Vec<char> = "two".chars().collect();

        assert_eq!(find_char_subslice(&haystack, &needle, 0), Some(4));
        assert_eq!(find_char_subslice(&haystack, &needle, 5), Some(8));
        assert_eq!(find_char_subslice(&haystack, &needle, 9), None);
        assert_eq!(find_char_subslice(&haystack, &[], 0), None);
        assert_eq!(find_char_subslice(&[], &needle, 0), None);
    }

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    #[test]
    fn utf16_offsets_map_back_to_char_indices() {
        let text = chars("😀 teh cat");
        assert_eq!(char_index_for_utf16(&text, 0), 0);
        // After the emoji's two UTF-16 units comes char 1.
        assert_eq!(char_index_for_utf16(&text, 2), 1);
        assert_eq!(char_index_for_utf16(&text, 3), 2);
        // An offset inside the surrogate pair rounds forward.
        assert_eq!(char_index_for_utf16(&text, 1), 1);
        assert_eq!(char_index_for_utf16(&text, 99), text.len());
    }

    #[test]
    fn utf16_ranges_count_surrogate_pairs() {
        let text = chars("😀 teh cat");
        // The emoji is one char but two UTF-16 units.
        assert_eq!(utf16_range(&text, 2, 5), (3, 3));
        assert_eq!(utf16_range(&text, 0, 1), (0, 2));
        // Out-of-range spans are clamped rather than panicking.
        assert_eq!(utf16_range(&text, 8, 50), (9, 1));
        assert_eq!(utf16_range(&text, 5, 2), (6, 0));
    }

    #[test]
    fn an_unchanged_field_keeps_the_span() {
        let text = chars("I saw teh cat.");
        assert_eq!(relocate_span(&text, &text, 6, 9), Some((6, 9)));
    }

    #[test]
    fn typing_after_the_span_keeps_it_in_place() {
        let source = chars("I saw teh cat.");
        let live = chars("I saw teh cat. And a dog.");
        assert_eq!(relocate_span(&source, &live, 6, 9), Some((6, 9)));
    }

    #[test]
    fn typing_before_the_span_moves_it() {
        let source = chars("I saw teh cat.");
        let live = chars("Yesterday I saw teh cat.");
        assert_eq!(relocate_span(&source, &live, 6, 9), Some((16, 19)));
    }

    #[test]
    fn a_span_whose_word_was_extended_is_not_found() {
        let source = chars("I saw teh cat.");
        let live = chars("I saw tehre cat.");
        assert_eq!(relocate_span(&source, &live, 6, 9), None);
    }

    #[test]
    fn a_span_that_was_deleted_is_not_found() {
        let source = chars("I saw teh cat.");
        let live = chars("I saw the cat.");
        assert_eq!(relocate_span(&source, &live, 6, 9), None);
    }

    #[test]
    fn an_ambiguous_move_is_refused() {
        let source = chars("teh a teh");
        let live = chars("xx teh a teh");
        assert_eq!(relocate_span(&source, &live, 0, 3), None);
    }

    #[test]
    fn severity_priority_orders_high_before_low() {
        assert!(
            severity_priority(&elocutio_core::Severity::High)
                < severity_priority(&elocutio_core::Severity::Medium)
        );
        assert!(
            severity_priority(&elocutio_core::Severity::Medium)
                < severity_priority(&elocutio_core::Severity::Low)
        );
    }

    #[test]
    fn text_range_bounds_probe_requires_non_zero_geometry() {
        let usable = TextRangeBoundsProbe::Success(Rect {
            x: 10.0,
            y: 20.0,
            width: 1.0,
            height: 12.0,
        });
        let unavailable = TextRangeBoundsProbe::Unavailable;

        assert!(usable.has_usable_text_metrics());
        assert!(!unavailable.has_usable_text_metrics());
        assert!(rect_has_usable_text_metrics(Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }));
        assert!(!rect_has_usable_text_metrics(Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 1.0,
        }));
    }
}
