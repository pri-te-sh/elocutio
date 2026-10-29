//! Local, deterministic tone detection.
//!
//! The app can *rewrite* in a chosen tone, but it cannot yet tell the writer
//! what tone they are already writing in. This module answers that question
//! **offline**: no network call, no model, no randomness. Same text in, same
//! signal out, every time.
//!
//! Every signal is a transparent lexical or structural measurement, and every
//! measurement that contributed is reported in [`ToneSignal::evidence`]. The
//! app's guiding principle is "Explain every edit" — a tone label the writer
//! cannot interrogate is not usable, so the evidence is part of the API, not an
//! afterthought.
//!
//! The detector is deliberately *conservative*: it only names a tone when the
//! evidence crosses a threshold, and it falls back to [`Tone::Neutral`]
//! otherwise. A mislabelled tone is worse than no label.

use serde::{Deserialize, Serialize};

/// The register of a passage.
///
/// The vocabulary matches the rewrite tones the app already offers
/// (`apps/macos/src-tauri/src/openrouter.rs`, `RewriteTone`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    /// No clear register signal, or signals that cancel out.
    #[default]
    Neutral,
    /// Direct, assertive, free of hedging.
    Confident,
    /// Warm and approachable.
    Friendly,
    /// Dense and short — few words per sentence, no filler.
    Concise,
    /// Distant, professional, impersonal.
    Formal,
}

impl Tone {
    /// Every tone, in a stable order suitable for a settings UI.
    pub const ALL: [Tone; 5] = [
        Tone::Neutral,
        Tone::Confident,
        Tone::Friendly,
        Tone::Concise,
        Tone::Formal,
    ];

    /// A short human-readable label.
    pub const fn label(self) -> &'static str {
        match self {
            Tone::Neutral => "Neutral",
            Tone::Confident => "Confident",
            Tone::Friendly => "Friendly",
            Tone::Concise => "Concise",
            Tone::Formal => "Formal",
        }
    }

    /// The wire name, matching the app's `RewriteTone` serde naming.
    pub const fn as_str(self) -> &'static str {
        match self {
            Tone::Neutral => "neutral",
            Tone::Confident => "confident",
            Tone::Friendly => "friendly",
            Tone::Concise => "concise",
            Tone::Formal => "formal",
        }
    }
}

/// The result of tone detection: a label, how sure the detector is, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToneSignal {
    /// The detected register.
    pub tone: Tone,
    /// How confident the detector is, in `0.0..=1.0`.
    ///
    /// This is a *heuristic* confidence derived from the strength of the
    /// winning signal, not a model-reported value.
    pub confidence: f64,
    /// Human-readable statements of the measurements that produced the label.
    ///
    /// Empty only when the text was empty. The app shows these so the writer
    /// can see *why* a tone was chosen.
    pub evidence: Vec<String>,
}

/// The maximum length, in characters, of a sanitized custom instruction.
pub const MAX_INSTRUCTION_LENGTH: usize = 280;

/// The minimum score a tone must reach before it is reported.
const MIN_SCORE: f64 = 0.55;

/// The raw lexical and structural measurements taken from a passage.
#[derive(Debug, Clone, Copy, Default)]
struct Measurements {
    word_count: usize,
    sentence_count: usize,
    contractions: usize,
    exclamations: usize,
    questions: usize,
    hedges: usize,
    intensifiers: usize,
    first_person: usize,
    second_person: usize,
    formal_markers: usize,
    informal_markers: usize,
    imperatives: usize,
    passive: usize,
    avg_sentence_words: f64,
}

impl Measurements {
    /// Count per thousand words, so thresholds do not depend on text length.
    fn per_thousand(&self, count: usize) -> f64 {
        count as f64 * 1000.0 / self.word_count.max(1) as f64
    }

    /// Render every measurement for the UI.
    fn evidence(&self) -> Vec<String> {
        vec![
            format!(
                "{} words in {} sentence(s)",
                self.word_count, self.sentence_count
            ),
            format!(
                "contractions: {} ({:.1} per 1k words)",
                self.contractions,
                self.per_thousand(self.contractions)
            ),
            format!("exclamation marks: {}", self.exclamations),
            format!("questions: {}", self.questions),
            format!(
                "hedges: {} ({:.1} per 1k words)",
                self.hedges,
                self.per_thousand(self.hedges)
            ),
            format!(
                "intensifiers: {} ({:.1} per 1k words)",
                self.intensifiers,
                self.per_thousand(self.intensifiers)
            ),
            format!(
                "first-person pronouns: {} ({:.1} per 1k words)",
                self.first_person,
                self.per_thousand(self.first_person)
            ),
            format!(
                "second-person pronouns: {} ({:.1} per 1k words)",
                self.second_person,
                self.per_thousand(self.second_person)
            ),
            format!(
                "formality markers: {} ({:.1} per 1k words)",
                self.formal_markers,
                self.per_thousand(self.formal_markers)
            ),
            format!(
                "informal markers: {} ({:.1} per 1k words)",
                self.informal_markers,
                self.per_thousand(self.informal_markers)
            ),
            format!(
                "imperative sentence starts: {} ({:.1} per 1k words)",
                self.imperatives,
                self.per_thousand(self.imperatives)
            ),
            format!(
                "passive constructions: {} ({:.1} per 1k words)",
                self.passive,
                self.per_thousand(self.passive)
            ),
            format!(
                "average sentence length: {:.1} words",
                self.avg_sentence_words
            ),
        ]
    }
}

/// Detect the register of `text`.
///
/// The detector is local and deterministic. It measures:
///
/// * contraction density,
/// * exclamation marks and other emphasis,
/// * hedges and intensifiers,
/// * first-person vs second-person pronouns,
/// * imperative sentence starts,
/// * passive voice,
/// * sentence length,
/// * formality markers,
/// * slang and filler.
///
/// and combines them into one label. Every measurement is listed in
/// `evidence`.
pub fn detect_tone(text: &str) -> ToneSignal {
    let text = text.trim();
    if text.is_empty() {
        return ToneSignal {
            tone: Tone::Neutral,
            confidence: 0.0,
            evidence: Vec::new(),
        };
    }

    let measurements = measure(text);

    let confident = confident_score(&measurements);
    let friendly = friendly_score(&measurements);
    let concise = concise_score(&measurements);
    let formal = formal_score(&measurements);

    let scores = [
        (Tone::Confident, confident),
        (Tone::Friendly, friendly),
        (Tone::Concise, concise),
        (Tone::Formal, formal),
    ];

    let (tone, best) = scores
        .iter()
        .copied()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((Tone::Neutral, 0.0));

    let mut evidence = measurements.evidence();

    if best < MIN_SCORE {
        evidence.insert(
            0,
            format!(
                "no register signal crossed the threshold (best score {:.2} < {MIN_SCORE:.2})",
                best
            ),
        );
        return ToneSignal {
            tone: Tone::Neutral,
            confidence: (best / MIN_SCORE).clamp(0.0, 0.5),
            evidence,
        };
    }

    evidence.insert(
        0,
        format!(
            "{} scored {:.2}, above the {:.2} threshold",
            tone.label(),
            best,
            MIN_SCORE
        ),
    );

    // Confidence scales with how far above the threshold the score is and how
    // far ahead of the runner-up it is.
    let runner_up = scores
        .iter()
        .copied()
        .filter(|(candidate, _)| *candidate != tone)
        .map(|(_, score)| score)
        .fold(0.0_f64, f64::max);
    let margin = (best - runner_up).clamp(0.0, 1.0);
    let confidence = ((best - MIN_SCORE) / (1.0 - MIN_SCORE) * 0.6 + margin * 0.4).clamp(0.0, 1.0);

    ToneSignal {
        tone,
        confidence,
        evidence,
    }
}

/// Score for [`Tone::Confident`]: direct, unhedged, first-person, active.
///
/// Absence of hedging alone is not confidence — a formal notice is also
/// unhedged. Confidence needs *assertion*: the writer in their own voice,
/// making claims, in the active voice.
fn confident_score(m: &Measurements) -> f64 {
    let mut score = 0.0f64;
    // Unhedged: necessary but not sufficient.
    if m.hedges == 0 {
        score += 0.20;
    } else if m.per_thousand(m.hedges) < 6.0 {
        score += 0.10;
    }
    // First person, in the writer's own voice.
    if m.per_thousand(m.first_person) >= 10.0 {
        score += 0.30;
    }
    // Active voice.
    if m.passive == 0 {
        score += 0.15;
    } else if m.per_thousand(m.passive) < 8.0 {
        score += 0.08;
    }
    // Assertive intensifiers without hedges.
    if m.per_thousand(m.intensifiers) >= 4.0 && m.hedges == 0 {
        score += 0.20;
    }
    // Imperative sentences: the writer telling, not asking.
    if m.per_thousand(m.imperatives) >= 4.0 {
        score += 0.15;
    }
    // Dense formal vocabulary is a formal register, not a confident one:
    // "we shall inform you accordingly" is ceremony, not directness.
    if m.per_thousand(m.formal_markers) >= 50.0 {
        score -= 0.30;
    }
    score.max(0.0)
}

/// Score for [`Tone::Friendly`]: warm, direct address, contractions, warmth.
fn friendly_score(m: &Measurements) -> f64 {
    let mut score = 0.0;
    // Contractions are the clearest sign of a conversational register.
    if m.per_thousand(m.contractions) >= 8.0 {
        score += 0.35;
    } else if m.per_thousand(m.contractions) >= 4.0 {
        score += 0.15;
    }
    // Direct address.
    if m.per_thousand(m.second_person) >= 6.0 {
        score += 0.25;
    }
    // Warmth: exclamation marks, questions, and informal vocabulary.
    if m.exclamations > 0 {
        score += 0.15;
    }
    if m.questions > 0 {
        score += 0.10;
    }
    if m.per_thousand(m.informal_markers) >= 3.0 {
        score += 0.15;
    }
    score
}

/// Score for [`Tone::Concise`]: short sentences, no filler, no ceremony.
///
/// Concision is the easiest register to fake — every short text has short
/// sentences. It therefore demands *both* terse sentences and the absence of
/// anything that pads them out, and it only applies to a body of text.
fn concise_score(m: &Measurements) -> f64 {
    // Concision is only meaningful over a body of text. A two-word fragment
    // has short sentences by default, and calling it "concise" would be
    // noise.
    if m.word_count < 20 {
        return 0.0;
    }

    let mut score = 0.0f64;
    // Short sentences are the defining feature.
    if m.avg_sentence_words <= 8.0 {
        score += 0.35;
    } else if m.avg_sentence_words <= 14.0 {
        score += 0.20;
    } else if m.avg_sentence_words <= 20.0 {
        score += 0.05;
    }
    // No filler at all: no hedges, no padding intensifiers, no ceremony.
    if m.hedges == 0 && m.per_thousand(m.intensifiers) < 3.0 && m.formal_markers == 0 {
        score += 0.30;
    } else {
        // Any one of those is enough to disqualify the text as "concise",
        // because a formal or padded sentence is exactly what the writer
        // asked to have removed.
        score -= 0.15;
    }
    if m.passive == 0 {
        score += 0.10;
    }
    // Dense: not padded with contractions and filler words.
    if m.per_thousand(m.contractions) < 3.0 {
        score += 0.10;
    }
    score.clamp(0.0, 1.0)
}

/// Score for [`Tone::Formal`]: professional vocabulary, impersonal, formal.
fn formal_score(m: &Measurements) -> f64 {
    let mut score = 0.0;
    // Professional and legal register vocabulary. This is the decisive
    // signal: dense formal vocabulary means a formal register, whatever
    // else the passage does.
    if m.per_thousand(m.formal_markers) >= 100.0 {
        score += 0.60;
    } else if m.per_thousand(m.formal_markers) >= 20.0 {
        score += 0.40;
    } else if m.per_thousand(m.formal_markers) >= 5.0 {
        score += 0.20;
    }
    // Impersonal: neither first nor second person.
    if m.per_thousand(m.first_person) < 4.0 && m.per_thousand(m.second_person) < 4.0 {
        score += 0.15;
    }
    // No contractions.
    if m.contractions == 0 {
        score += 0.15;
    }
    // No informal vocabulary.
    if m.informal_markers == 0 {
        score += 0.10;
    }
    score
}

/// Take every measurement from the text.
fn measure(text: &str) -> Measurements {
    // macOS smart quotes turn "don't" into "don’t"; fold the typographic
    // apostrophes so contractions and word lists match either spelling.
    let text = &normalize_apostrophes(text);
    let lower = text.to_lowercase();
    let words = tokenize(&lower);
    let sentences = split_sentences(text);
    let word_count = words.len();

    let avg_sentence_words = if sentences.is_empty() {
        word_count as f64
    } else {
        word_count as f64 / sentences.len() as f64
    };

    Measurements {
        word_count,
        sentence_count: sentences.len(),
        contractions: count_contractions(&words),
        exclamations: text.matches('!').count(),
        questions: text.matches('?').count(),
        hedges: count_any(&words, HEDGES),
        intensifiers: count_any(&words, INTENSIFIERS),
        first_person: count_any(&words, FIRST_PERSON),
        second_person: count_any(&words, SECOND_PERSON),
        formal_markers: count_any(&words, FORMAL_MARKERS),
        informal_markers: count_any(&words, INFORMAL_MARKERS),
        imperatives: count_imperatives(&sentences),
        passive: count_passive(&sentences),
        avg_sentence_words,
    }
}

/// Prompt guidance for a tone.
///
/// The wording matches the app's existing rewrite guidance in
/// `apps/macos/src-tauri/src/openrouter.rs` (`RewriteTone::guidance`) so the
/// detected tone and the rewrite tone speak the same language.
pub fn tone_guidance(tone: Tone) -> Option<&'static str> {
    match tone {
        Tone::Neutral => None,
        Tone::Confident => Some("Use a confident, direct tone without hedging."),
        Tone::Friendly => Some("Use a warm, friendly tone while staying professional."),
        Tone::Concise => Some("Be concise; cut filler and unnecessary qualifiers."),
        Tone::Formal => Some("Use a formal, precise register."),
    }
}

/// Sanitize a free-text custom instruction for use in a prompt.
///
/// The result is safe to embed in a prompt:
///
/// * control characters are stripped,
/// * the text is collapsed to a single line (no prompt smuggling via
///   newlines),
/// * it is truncated to [`MAX_INSTRUCTION_LENGTH`] characters,
/// * angle brackets are removed, so the text cannot close the wrapper tag and
///   escape into the rest of the prompt,
/// * it is wrapped in a data marker so the model reads it as content, not as
///   instructions.
///
/// Returns `None` when nothing usable is left.
pub fn custom_instruction(text: &str) -> Option<String> {
    let cleaned: String = text
        .chars()
        .filter(|character| !character.is_control() || *character == '\t')
        .map(|character| if character == '\t' { ' ' } else { character })
        .collect();

    let mut collapsed = String::with_capacity(cleaned.len());
    let mut previous_space = false;
    for character in cleaned.chars() {
        if character.is_whitespace() {
            if !previous_space {
                collapsed.push(' ');
            }
            previous_space = true;
        } else {
            collapsed.push(character);
            previous_space = false;
        }
    }

    let collapsed = collapsed.trim();
    if collapsed.is_empty() {
        return None;
    }

    let truncated: String = collapsed.chars().take(MAX_INSTRUCTION_LENGTH).collect();
    let truncated = truncated.trim_end();
    if truncated.is_empty() {
        return None;
    }

    // The instruction is wrapped in a tag, so the text must not be able to
    // close that tag. Angle brackets are the only characters that can do it.
    let safe: String = truncated
        .chars()
        .filter(|character| *character != '<')
        .collect();

    Some(format!(
        "<custom_instruction data=\"true\">{safe}</custom_instruction>"
    ))
}

/// Hedge words: uncertainty markers that soften a claim.
const HEDGES: &[&str] = &[
    "maybe",
    "perhaps",
    "possibly",
    "probably",
    "might",
    "could",
    "seems",
    "seem",
    "somewhat",
    "sort",
    "kind",
    "fairly",
    "rather",
    "arguably",
    "allegedly",
    "apparently",
    "presumably",
    "supposedly",
    "roughly",
    "approximately",
    "perhaps",
    "slightly",
    "marginally",
];

/// Intensifiers: words that amplify a claim.
const INTENSIFIERS: &[&str] = &[
    "very",
    "really",
    "extremely",
    "incredibly",
    "absolutely",
    "completely",
    "totally",
    "definitely",
    "certainly",
    "clearly",
    "obviously",
    "always",
    "never",
    "must",
    "will",
    "shall",
    "utterly",
    "highly",
    "deeply",
    "truly",
    "genuinely",
];

/// First-person pronouns.
const FIRST_PERSON: &[&str] = &[
    "i", "me", "my", "mine", "we", "us", "our", "ours", "i've", "i'm", "i'll", "i'd",
];

/// Second-person pronouns.
const SECOND_PERSON: &[&str] = &["you", "your", "yours", "yourself", "yourselves"];

/// Formal markers: vocabulary typical of professional or legal registers.
const FORMAL_MARKERS: &[&str] = &[
    "therefore",
    "hereby",
    "accordingly",
    "furthermore",
    "moreover",
    "nevertheless",
    "nonetheless",
    "pursuant",
    "regarding",
    "aforementioned",
    "commence",
    "terminate",
    "inquire",
    "utilize",
    "kindly",
    "grateful",
    "submit",
    "compliance",
    "notwithstanding",
    "please",
];

/// Informal markers: slang, filler, and conversational register.
const INFORMAL_MARKERS: &[&str] = &[
    "gonna", "wanna", "kinda", "yeah", "yep", "nope", "cool", "awesome", "dude", "hey", "lol",
    "stuff", "things", "super", "guys", "ok", "okay", "alright", "cheers", "mate",
];

/// Contractions, listed so they can be counted without regex.
const CONTRACTIONS: &[&str] = &[
    "don't",
    "doesn't",
    "didn't",
    "isn't",
    "aren't",
    "wasn't",
    "weren't",
    "can't",
    "cannot",
    "won't",
    "wouldn't",
    "shouldn't",
    "couldn't",
    "mustn't",
    "haven't",
    "hasn't",
    "hadn't",
    "i'm",
    "i've",
    "i'll",
    "i'd",
    "you're",
    "you've",
    "you'll",
    "you'd",
    "he's",
    "she's",
    "it's",
    "we're",
    "we've",
    "we'll",
    "they're",
    "they've",
    "they'll",
    "that's",
    "there's",
    "here's",
    "what's",
    "let's",
    "ain't",
    "gonna",
    "wanna",
    "gotta",
];

/// Words that commonly begin an imperative sentence.
const IMPERATIVE_STARTERS: &[&str] = &[
    "use",
    "add",
    "remove",
    "replace",
    "write",
    "read",
    "check",
    "click",
    "open",
    "close",
    "send",
    "call",
    "email",
    "save",
    "choose",
    "select",
    "enter",
    "type",
    "press",
    "do",
    "don't",
    "try",
    "keep",
    "put",
    "place",
    "set",
    "start",
    "stop",
    "run",
    "install",
    "ensure",
    "verify",
    "review",
    "update",
    "uninstall",
    "configure",
    "connect",
    "disconnect",
    "refresh",
    "restart",
    "navigate",
    "scroll",
    "tap",
    "swipe",
    "copy",
    "paste",
    "avoid",
    "consider",
    "remember",
    "forget",
];

/// Tokens that mark a passive construction.
const PASSIVE_MARKERS: &[&str] = &["was", "were", "is", "are", "been", "being", "be"];

/// Replace typographic apostrophes (U+2019, U+02BC) with ASCII `'`.
fn normalize_apostrophes(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains(['\u{2019}', '\u{02BC}']) {
        std::borrow::Cow::Owned(text.replace(['\u{2019}', '\u{02BC}'], "'"))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// Split text into lowercase word-ish tokens. Contractions keep the apostrophe.
fn tokenize(lower: &str) -> Vec<&str> {
    lower
        .split(|character: char| !character.is_alphanumeric() && character != '\'')
        .filter(|token| !token.is_empty())
        .collect()
}

/// Count tokens that appear in `list`.
fn count_any(words: &[&str], list: &[&str]) -> usize {
    words.iter().filter(|word| list.contains(word)).count()
}

/// Count contractions, including any token with an internal apostrophe.
fn count_contractions(words: &[&str]) -> usize {
    words
        .iter()
        .filter(|word| CONTRACTIONS.contains(word) || word.contains('\''))
        .count()
}

/// Split text into sentences.
fn split_sentences(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();

    for character in text.chars() {
        current.push(character);
        if matches!(character, '.' | '!' | '?' | '\n' | ';' | '…') {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                result.push(trimmed.to_string());
            }
            current.clear();
        }
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        result.push(trimmed.to_string());
    }

    result
}

/// Count sentences that begin with an imperative verb.
fn count_imperatives(sentences: &[String]) -> usize {
    sentences
        .iter()
        .filter(|sentence| {
            let first = sentence
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|character: char| !character.is_alphanumeric() && character != '\'')
                .to_lowercase();
            IMPERATIVE_STARTERS.contains(&first.as_str())
        })
        .count()
}

/// Count passive constructions ("is done", "was written", …).
fn count_passive(sentences: &[String]) -> usize {
    sentences
        .iter()
        .map(|sentence| {
            let words: Vec<String> = sentence
                .split_whitespace()
                .map(|word| {
                    word.trim_matches(|character: char| {
                        !character.is_alphanumeric() && character != '\''
                    })
                    .to_lowercase()
                })
                .collect();
            words
                .windows(2)
                .filter(|pair| {
                    PASSIVE_MARKERS.contains(&pair[0].as_str())
                        && (pair[1].ends_with("ed") || pair[1].ends_with("en"))
                })
                .count()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_neutral_with_no_evidence() {
        let signal = detect_tone("   ");
        assert_eq!(signal.tone, Tone::Neutral);
        assert_eq!(signal.confidence, 0.0);
        assert!(signal.evidence.is_empty());
    }

    #[test]
    fn curly_apostrophes_count_as_contractions() {
        let straight = "Hey! We're so glad you're here, and we don't mind. It's great.";
        let curly = straight.replace('\'', "\u{2019}");
        assert_eq!(detect_tone(&curly), detect_tone(straight));
        let measured = measure("don\u{2019}t won\u{02BC}t");
        assert_eq!(measured.word_count, 2);
        assert_eq!(measured.contractions, 2);
    }

    #[test]
    fn detection_is_deterministic() {
        let text = "We should ship this today! It works, and we're excited.";
        assert_eq!(detect_tone(text), detect_tone(text));
    }

    #[test]
    fn friendly_text_is_detected_as_friendly() {
        let text = "Hey there! We're so glad you're here. How can I help you today? \
                    It's a great day, and we think you'll love it.";
        let signal = detect_tone(text);
        assert_eq!(
            signal.tone,
            Tone::Friendly,
            "evidence: {:?}",
            signal.evidence
        );
    }

    #[test]
    fn formal_text_is_detected_as_formal() {
        let text = "Please utilize the attached form. Kindly submit your request to the \
                    compliance office regarding the aforementioned matter. We shall inform you \
                    accordingly. The department will commence a review pursuant to policy.";
        let signal = detect_tone(text);
        assert_eq!(signal.tone, Tone::Formal, "evidence: {:?}", signal.evidence);
    }

    #[test]
    fn concise_text_is_detected_as_concise() {
        let text = "Ship it. Test it. Ship it. We ship. The team ships fast. \
                    We test often. It works. We iterate. It ships.";
        let signal = detect_tone(text);
        assert_eq!(
            signal.tone,
            Tone::Concise,
            "evidence: {:?}",
            signal.evidence
        );
    }

    #[test]
    fn confident_text_is_detected_as_confident() {
        let text = "We built the thing. It works. We shipped it. We know it works. \
                    We built it for speed, and we will keep making it faster.";
        let signal = detect_tone(text);
        assert_eq!(
            signal.tone,
            Tone::Confident,
            "evidence: {:?}",
            signal.evidence
        );
    }

    #[test]
    fn plain_text_falls_back_to_neutral() {
        let text = "The report was written on Tuesday. It contains three sections. \
                    The second section covers revenue. The third covers costs.";
        let signal = detect_tone(text);
        assert_eq!(
            signal.tone,
            Tone::Neutral,
            "evidence: {:?}",
            signal.evidence
        );
    }

    #[test]
    fn evidence_is_always_populated_for_non_empty_text() {
        let signal = detect_tone("Some text.");
        assert!(!signal.evidence.is_empty());
        assert!(signal.evidence.len() > 5);
    }

    #[test]
    fn confidence_is_bounded() {
        for text in [
            "Hey!! We're so happy!!! Are you excited? It's awesome!",
            "Please utilize the aforementioned form regarding compliance.",
            "A short sentence.",
        ] {
            let signal = detect_tone(text);
            assert!(
                (0.0..=1.0).contains(&signal.confidence),
                "{} out of range",
                signal.confidence
            );
        }
    }

    #[test]
    fn guidance_matches_the_app_wording() {
        assert_eq!(tone_guidance(Tone::Neutral), None);
        assert_eq!(
            tone_guidance(Tone::Confident),
            Some("Use a confident, direct tone without hedging.")
        );
        assert_eq!(
            tone_guidance(Tone::Friendly),
            Some("Use a warm, friendly tone while staying professional.")
        );
        assert_eq!(
            tone_guidance(Tone::Concise),
            Some("Be concise; cut filler and unnecessary qualifiers.")
        );
        assert_eq!(
            tone_guidance(Tone::Formal),
            Some("Use a formal, precise register.")
        );
    }

    #[test]
    fn every_tone_has_a_label_and_a_wire_name() {
        for tone in Tone::ALL {
            assert!(!tone.label().is_empty());
            assert!(!tone.as_str().is_empty());
        }
        assert_eq!(Tone::ALL.len(), 5);
    }

    #[test]
    fn custom_instruction_strips_control_characters() {
        let instruction = custom_instruction("Make it\u{0}\u{7}shorter\nplease").unwrap();
        assert!(!instruction.contains('\u{0}'));
        assert!(!instruction.contains('\u{7}'));
        assert!(!instruction.contains('\n'));
    }

    #[test]
    fn custom_instruction_is_bounded() {
        let long = "a".repeat(MAX_INSTRUCTION_LENGTH * 3);
        let instruction = custom_instruction(&long).unwrap();
        let body = instruction
            .trim_start_matches("<custom_instruction data=\"true\">")
            .trim_end_matches("</custom_instruction>");
        assert!(body.chars().count() <= MAX_INSTRUCTION_LENGTH);
    }

    #[test]
    fn custom_instruction_marks_its_payload_as_data() {
        let instruction = custom_instruction("be brief").unwrap();
        assert!(instruction.starts_with("<custom_instruction data=\"true\">"));
        assert!(instruction.ends_with("</custom_instruction>"));
    }

    #[test]
    fn custom_instruction_rejects_empty_input() {
        assert_eq!(custom_instruction(""), None);
        assert_eq!(custom_instruction("   \n\t  "), None);
    }

    #[test]
    fn custom_instruction_collapses_whitespace() {
        let instruction = custom_instruction("  make   it\t\tshorter  ").unwrap();
        assert!(instruction.contains("make it shorter"));
    }

    #[test]
    fn tone_serializes_to_snake_case() {
        let json = serde_json::to_string(&Tone::Friendly).unwrap();
        assert_eq!(json, "\"friendly\"");
    }
}
