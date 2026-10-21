#![forbid(unsafe_code)]

//! Chunking, prompt/schema, and quote-to-offset resolution for Elocutio.
//!
//! Models return verbatim quotes plus paragraph ids, never offsets. This crate
//! locates those quotes (exact → normalized → fuzzy), computes UTF-16 and
//! grapheme spans, validates the model payload, and drops ambiguous edits.
//!
//! It also provides the local, deterministic analysis that needs no network:
//! [`tone`] detects the register of a passage, [`merge`] reconciles local
//! lints with model suggestions, and [`golden`] scores a run against a
//! labelled golden set.

pub mod chunk;
pub mod diff;
pub mod golden;
pub mod merge;
pub mod parse;
pub mod prompt;
pub mod resolve;
pub mod tone;

pub use chunk::{Paragraph, chunk_paragraphs};
pub use diff::{DiffSegment, diff_words};
pub use golden::{
    GoldenItem, GoldenMetrics, GoldenReport, GoldenSet, Prediction, evaluate, evaluate_resolver,
    load_from_json,
};
pub use merge::{
    DEFAULT_SUGGESTION_LIMIT, DropReason, DroppedSuggestion, MergeReport, MergeResult,
    SuggestionSource, cap_suggestions, merge, merge_with_limit, rank_suggestions,
};
pub use parse::{AnalysisResult, RejectedEdit, parse_response};
pub use prompt::{
    PROMPT_VERSION, SCHEMA_VERSION, build_user_prompt, build_user_prompt_with_context, json_schema,
    system_prompt,
};
pub use resolve::{RawEdit, ResolveError, resolve_edit, resolve_exact};
pub use tone::{Tone, ToneSignal, custom_instruction, detect_tone, tone_guidance};

// The core domain types travel with the analysis API, so a caller only needs
// one import to work with a suggestion end to end.
pub use elocutio_core::{Category, Resolution, Severity, Suggestion, TextSpan};
