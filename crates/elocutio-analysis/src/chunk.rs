//! Paragraph chunking with stable ids and original byte ranges.

use serde::{Deserialize, Serialize};

/// A paragraph sent to the model. `text` is an exact substring of the input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paragraph {
    pub id: String,
    pub text: String,
    pub byte_start: usize,
    pub byte_end: usize,
}

/// Split text into paragraphs separated by one or more blank lines.
///
/// IDs are `p1`, `p2`, … and are stable for a given input. Leading/trailing
/// whitespace is trimmed from each paragraph, but the returned byte range still
/// points into the original string.
pub fn chunk_paragraphs(text: &str) -> Vec<Paragraph> {
    let mut paragraphs = Vec::new();
    let mut paragraph_start = 0;
    let mut cursor = 0;
    let mut in_paragraph = false;

    while cursor < text.len() {
        let line_end = text[cursor..]
            .find('\n')
            .map(|offset| cursor + offset + 1)
            .unwrap_or(text.len());
        let line = &text[cursor..line_end];

        if line.trim().is_empty() {
            if in_paragraph && cursor > paragraph_start {
                push_paragraph(text, paragraph_start, cursor, &mut paragraphs);
            }
            in_paragraph = false;
            paragraph_start = line_end;
        } else if !in_paragraph {
            in_paragraph = true;
            paragraph_start = cursor;
        }

        cursor = line_end;
    }

    if in_paragraph && text.len() > paragraph_start {
        push_paragraph(text, paragraph_start, text.len(), &mut paragraphs);
    }

    paragraphs
}

fn push_paragraph(text: &str, start: usize, end: usize, out: &mut Vec<Paragraph>) {
    let slice = &text[start..end];
    let trimmed = slice.trim();
    if trimmed.is_empty() {
        return;
    }

    let leading = slice.len() - slice.trim_start().len();
    let byte_start = start + leading;
    let byte_end = byte_start + trimmed.len();
    let id = format!("p{}", out.len() + 1);

    out.push(Paragraph {
        id,
        text: trimmed.to_string(),
        byte_start,
        byte_end,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_blank_line_paragraphs() {
        let text = "First paragraph.\n\nSecond paragraph.\n\nThird.";
        let paragraphs = chunk_paragraphs(text);
        assert_eq!(paragraphs.len(), 3);
        assert_eq!(paragraphs[0].id, "p1");
        assert_eq!(paragraphs[1].text, "Second paragraph.");
        assert_eq!(paragraphs[2].id, "p3");
    }

    #[test]
    fn preserves_byte_ranges_into_original() {
        let text = "  Hello world.  \n\nNext.";
        let paragraphs = chunk_paragraphs(text);
        assert_eq!(
            &text[paragraphs[0].byte_start..paragraphs[0].byte_end],
            "Hello world."
        );
        assert_eq!(
            &text[paragraphs[1].byte_start..paragraphs[1].byte_end],
            "Next."
        );
    }

    #[test]
    fn handles_crlf_and_single_newlines() {
        let text = "Line one\nline two\r\n\r\nNew paragraph.";
        let paragraphs = chunk_paragraphs(text);
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(paragraphs[0].text, "Line one\nline two");
        assert_eq!(paragraphs[1].text, "New paragraph.");
    }

    #[test]
    fn empty_text_has_no_paragraphs() {
        assert!(chunk_paragraphs("").is_empty());
        assert!(chunk_paragraphs(" \n\n \n").is_empty());
    }
}
