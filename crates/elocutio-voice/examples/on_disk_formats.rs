//! Print the exact on-disk JSON shapes for the voice profile and the feedback
//! log, so the privacy documentation can be written against reality.
//!
//! Run with: `cargo run -p elocutio-voice --example on_disk_formats`

use elocutio_voice::{
    EditOrigin, StoredSuggestionEvent, SuggestionEvent, SuggestionEventKind, TraitValue,
    VoiceContext, VoiceEvent, VoiceStrength, add_sample_guards, analyze_samples,
    default_fingerprint_size, set_trait_override,
};

fn main() {
    let sample = "Hi Sam, quick update on the launch. We are on track for Thursday. Thanks!";

    let mut profile = analyze_samples(
        "voice-1700000000",
        "My voice",
        VoiceContext::General,
        &[sample.to_string()],
    );
    profile.strength = VoiceStrength::Balanced;
    add_sample_guards(&mut profile, sample, default_fingerprint_size());
    set_trait_override(&mut profile, "avg_sentence_words", TraitValue::Number(9.0));

    println!("=== voice-profile.json ===");
    println!(
        "{}",
        serde_json::to_string_pretty(&profile).expect("profile serializes")
    );

    println!();
    println!("=== voice-feedback.jsonl (one JSON object per line) ===");
    for event in [
        SuggestionEvent {
            kind: SuggestionEventKind::Accepted,
            category: "clarity".into(),
            original: "we are going".into(),
            replacement: "we're going".into(),
            final_text: String::new(),
            origin: EditOrigin::User,
            app: "com.apple.mail".into(),
            timestamp: 1_700_000_000,
        },
        SuggestionEvent {
            kind: SuggestionEventKind::Dismissed,
            category: "word_choice".into(),
            original: "utilize".into(),
            replacement: "use".into(),
            final_text: String::new(),
            origin: EditOrigin::User,
            app: "com.apple.mail".into(),
            timestamp: 1_700_000_100,
        },
        SuggestionEvent {
            kind: SuggestionEventKind::EditedAfterAccept,
            category: "style".into(),
            original: "in order to".into(),
            replacement: "to".into(),
            final_text: "so".into(),
            origin: EditOrigin::Model,
            app: "com.apple.mail".into(),
            timestamp: 1_700_000_200,
        },
    ] {
        let stored = StoredSuggestionEvent::from_event(&event);
        println!(
            "{}",
            serde_json::to_string(&stored).expect("event serializes")
        );
    }

    println!();
    println!("=== voice-feedback.jsonl (legacy line, still parsed) ===");
    let legacy =
        r#"{"original":"a","replacement":"b","category":"clarity","timestamp":1700000000}"#;
    println!("{legacy}");

    println!();
    println!("=== voice-events.jsonl (one JSON object per line) ===");
    for event in [
        VoiceEvent::rated(
            elocutio_voice::Rating::ThumbsUp,
            Some("email".into()),
            1_700_000_000,
        ),
        VoiceEvent::tone_selected("friendly", Some("email".into()), 1_700_000_100),
        VoiceEvent::override_set("avg_sentence_words", 9.0, None, 1_700_000_200),
    ] {
        println!(
            "{}",
            serde_json::to_string(&event).expect("voice event serializes")
        );
    }

    println!();
    println!("=== voice-learning.enabled ===");
    println!("(file contents: the single byte '1')");
}
