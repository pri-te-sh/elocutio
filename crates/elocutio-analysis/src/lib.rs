#![forbid(unsafe_code)]

//! Chunking, prompt/schema, and quote-to-offset resolution for Elocutio.
//!
//! Models return verbatim quotes plus paragraph ids, never offsets. This crate
//! locates those quotes (exact → normalized → fuzzy), computes UTF-16 and
//! grapheme spans, validates the model payload, and drops ambiguous edits.

pub mod chunk;
pub mod parse;
pub mod prompt;
pub mod resolve;

pub use chunk::{Paragraph, chunk_paragraphs};
pub use parse::{AnalysisResult, RejectedEdit, parse_response};
pub use prompt::{PROMPT_VERSION, SCHEMA_VERSION, build_user_prompt, json_schema, system_prompt};
pub use resolve::{RawEdit, ResolveError, resolve_edit, resolve_exact};
