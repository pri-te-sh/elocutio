#![forbid(unsafe_code)]

//! Core domain types for Elocutio.
//!
//! These types are shared by the analysis engine, the provider layer, the
//! voice engine, and the UI. They intentionally contain no I/O so they can be
//! tested in isolation and reused by the future CLI.

pub mod suggestion;

pub use suggestion::{Category, Resolution, Severity, Suggestion, TextSpan};
