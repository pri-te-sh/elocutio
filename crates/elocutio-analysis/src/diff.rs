//! Word-level diff for rewrite previews.

use serde::{Deserialize, Serialize};

/// One aligned run in a word diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum DiffSegment {
    Equal(String),
    Insert(String),
    Delete(String),
}

/// Diff two strings by whitespace-delimited words.
///
/// The result is coalesced so adjacent runs of the same kind appear once. Large
/// inputs fall back to a single delete/insert pair rather than allocating a huge
/// LCS table.
pub fn diff_words(before: &str, after: &str) -> Vec<DiffSegment> {
    let before_tokens: Vec<&str> = before.split_whitespace().collect();
    let after_tokens: Vec<&str> = after.split_whitespace().collect();

    if before_tokens == after_tokens {
        return if before_tokens.is_empty() {
            Vec::new()
        } else {
            vec![DiffSegment::Equal(before_tokens.join(" "))]
        };
    }

    if before_tokens.len().saturating_mul(after_tokens.len()) > 1_000_000 {
        return vec![
            DiffSegment::Delete(before_tokens.join(" ")),
            DiffSegment::Insert(after_tokens.join(" ")),
        ];
    }

    let rows = before_tokens.len() + 1;
    let columns = after_tokens.len() + 1;
    let mut lcs = vec![vec![0u32; columns]; rows];
    for i in (0..before_tokens.len()).rev() {
        for j in (0..after_tokens.len()).rev() {
            lcs[i][j] = if before_tokens[i] == after_tokens[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut runs: Vec<DiffSegment> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < before_tokens.len() && j < after_tokens.len() {
        if before_tokens[i] == after_tokens[j] {
            push_run(&mut runs, DiffSegment::Equal(before_tokens[i].to_string()));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            push_run(&mut runs, DiffSegment::Delete(before_tokens[i].to_string()));
            i += 1;
        } else {
            push_run(&mut runs, DiffSegment::Insert(after_tokens[j].to_string()));
            j += 1;
        }
    }
    while i < before_tokens.len() {
        push_run(&mut runs, DiffSegment::Delete(before_tokens[i].to_string()));
        i += 1;
    }
    while j < after_tokens.len() {
        push_run(&mut runs, DiffSegment::Insert(after_tokens[j].to_string()));
        j += 1;
    }

    runs
}

fn push_run(runs: &mut Vec<DiffSegment>, segment: DiffSegment) {
    match (runs.last_mut(), &segment) {
        (Some(DiffSegment::Equal(existing)), DiffSegment::Equal(text)) => {
            existing.push(' ');
            existing.push_str(text);
        }
        (Some(DiffSegment::Insert(existing)), DiffSegment::Insert(text)) => {
            existing.push(' ');
            existing.push_str(text);
        }
        (Some(DiffSegment::Delete(existing)), DiffSegment::Delete(text)) => {
            existing.push(' ');
            existing.push_str(text);
        }
        _ => runs.push(segment),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_is_one_equal_run() {
        assert_eq!(
            diff_words("one two", "one two"),
            vec![DiffSegment::Equal("one two".into())]
        );
    }

    #[test]
    fn replacement_produces_delete_and_insert() {
        let diff = diff_words("Tehre is a test", "There is a test");
        assert!(diff.contains(&DiffSegment::Delete("Tehre".into())));
        assert!(diff.contains(&DiffSegment::Insert("There".into())));
        assert!(diff.contains(&DiffSegment::Equal("is a test".into())));
    }

    #[test]
    fn insertion_only() {
        let diff = diff_words("a test", "a short test");
        assert!(diff.contains(&DiffSegment::Insert("short".into())));
    }

    #[test]
    fn empty_before_is_an_insert() {
        assert_eq!(
            diff_words("", "new"),
            vec![DiffSegment::Insert("new".into())]
        );
    }
}
