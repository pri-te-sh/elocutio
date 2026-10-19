#![forbid(unsafe_code)]

//! Protocol shared by the Elocutio app and its browser extension.
//!
//! The first transport is a loopback WebSocket with a pairing token; native
//! messaging can replace it later without changing these messages.

use elocutio_core::Suggestion;
use serde::{Deserialize, Serialize};

/// Current bridge protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeMessage {
    Hello {
        token: String,
        client: String,
        version: String,
    },
    Analyze {
        id: String,
        site: String,
        text: String,
        selection: Option<Selection>,
    },
    Result {
        id: String,
        suggestions: Vec<Suggestion>,
    },
    Event {
        id: String,
        kind: String,
        suggestion_id: String,
    },
    Error {
        id: String,
        message: String,
    },
    Ping,
}

#[cfg(test)]
mod tests {
    use super::*;
    use elocutio_core::{Category, Resolution, Severity, TextSpan};

    #[test]
    fn hello_round_trips() {
        let message = BridgeMessage::Hello {
            token: "pair-token".into(),
            client: "chrome".into(),
            version: "0.1.0".into(),
        };
        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("\"type\":\"hello\""));
        let decoded: BridgeMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(message, decoded);
    }

    #[test]
    fn result_carries_suggestions() {
        let suggestion = Suggestion {
            id: "s1".into(),
            span: TextSpan::new(0, 4, 0, 4),
            original: "teh ".into(),
            replacement: "the ".into(),
            category: Category::Spelling,
            severity: Severity::High,
            explanation: "Misspelling.".into(),
            resolution: Resolution::Exact,
        };
        let message = BridgeMessage::Result {
            id: "r1".into(),
            suggestions: vec![suggestion],
        };
        let json = serde_json::to_string(&message).unwrap();
        let decoded: BridgeMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(message, decoded);
    }
}
