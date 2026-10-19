//! Local voice profile storage. Only the aggregate style card is written; raw
//! uploaded samples are discarded after analysis.

use std::path::PathBuf;

use elocutio_voice::{
    VoiceContext, VoiceProfile, VoiceStrength, analyze_samples, voice_prompt_guidance,
};
use serde::{Deserialize, Serialize};

use crate::config::Config;

fn path() -> Result<PathBuf, String> {
    Config::folder_path()
        .map(|folder| folder.join("voice-profile.json"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

fn learning_flag_path() -> Result<PathBuf, String> {
    Config::folder_path()
        .map(|folder| folder.join("voice-learning.enabled"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

fn feedback_path() -> Result<PathBuf, String> {
    Config::folder_path()
        .map(|folder| folder.join("voice-feedback.jsonl"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AcceptedEdit {
    original: String,
    replacement: String,
    category: String,
    timestamp: u64,
}

pub fn set_learning_enabled(enabled: bool) -> Result<(), String> {
    let path = learning_flag_path()?;
    if enabled {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create the voice directory: {error}"))?;
        }
        std::fs::write(&path, b"1")
            .map_err(|error| format!("Could not enable edit learning: {error}"))
    } else {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Could not disable edit learning: {error}")),
        }
    }
}

pub fn is_learning_enabled() -> bool {
    learning_flag_path()
        .map(|path| path.exists())
        .unwrap_or(false)
}

pub fn record_accepted(original: &str, replacement: &str, category: &str) -> Result<(), String> {
    if !is_learning_enabled() || original == replacement {
        return Ok(());
    }

    let path = feedback_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the voice directory: {error}"))?;
    }

    let entry = AcceptedEdit {
        original: original.to_string(),
        replacement: replacement.to_string(),
        category: category.to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0),
    };
    let line = serde_json::to_string(&entry).map_err(|error| error.to_string())?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Could not open voice feedback: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("Could not record accepted edit: {error}"))
}

pub fn refresh_feedback() -> Result<Option<VoiceProfile>, String> {
    let Some(mut profile) = load()? else {
        return Ok(None);
    };
    if learn_from_feedback(&mut profile)? > 0 {
        save(&profile)?;
    }
    Ok(Some(profile))
}

pub fn feedback_count() -> usize {
    feedback_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|contents| {
            contents
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count()
        })
        .unwrap_or(0)
}

fn learn_from_feedback(profile: &mut VoiceProfile) -> Result<usize, String> {
    let path = feedback_path()?;
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!("Could not read voice feedback: {error}")),
    };
    let entries: Vec<AcceptedEdit> = contents
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    if entries.is_empty() {
        return Ok(0);
    }

    let mut contractions_added = 0usize;
    let mut contractions_removed = 0usize;
    let mut shorter = 0usize;
    for entry in &entries {
        let original = entry.original.to_lowercase();
        let replacement = entry.replacement.to_lowercase();
        let original_has = has_contraction(&original);
        let replacement_has = has_contraction(&replacement);
        if !original_has && replacement_has {
            contractions_added += 1;
        } else if original_has && !replacement_has {
            contractions_removed += 1;
        }
        if replacement.chars().count() < original.chars().count() {
            shorter += 1;
        }
    }

    let count = entries.len();
    profile.traits.insert(
        "accepted_edits".to_string(),
        elocutio_voice::VoiceTrait {
            value: elocutio_voice::TraitValue::Number(count as f64),
            confidence: 0.9,
            evidence: vec![format!("{count} accepted edits")],
        },
    );
    if contractions_added + contractions_removed > 0 {
        profile.traits.insert(
            "prefers_contractions".to_string(),
            elocutio_voice::VoiceTrait {
                value: elocutio_voice::TraitValue::Bool(contractions_added > contractions_removed),
                confidence: 0.65,
                evidence: vec![format!(
                    "{contractions_added} added, {contractions_removed} removed"
                )],
            },
        );
    }
    if count >= 3 {
        profile.traits.insert(
            "prefers_concise".to_string(),
            elocutio_voice::VoiceTrait {
                value: elocutio_voice::TraitValue::Bool(shorter * 2 > count),
                confidence: 0.6,
                evidence: vec![format!("{shorter} of {count} shortened")],
            },
        );
    }

    Ok(count)
}

fn has_contraction(text: &str) -> bool {
    text.split_whitespace().any(|word| {
        ["n't", "'ll", "'re", "'ve", "'m", "'d", "'s"]
            .iter()
            .any(|suffix| word.ends_with(suffix))
    })
}

pub fn load() -> Result<Option<VoiceProfile>, String> {
    let path = path()?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read the voice profile: {error}")),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("Invalid voice profile: {error}"))
}

pub fn save(profile: &VoiceProfile) -> Result<(), String> {
    let path = path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the voice directory: {error}"))?;
    }
    std::fs::write(
        &path,
        serde_json::to_vec(profile).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("Could not write the voice profile: {error}"))
}

pub fn clear() -> Result<(), String> {
    let profile = path()?;
    match std::fs::remove_file(&profile) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Could not clear the voice profile: {error}")),
    }
    if let Ok(feedback) = feedback_path() {
        let _ = std::fs::remove_file(feedback);
    }
    let _ = set_learning_enabled(false);
    Ok(())
}

pub fn learn(text: &str, name: Option<String>) -> Result<VoiceProfile, String> {
    if text.trim().is_empty() {
        return Err("Paste at least a few paragraphs of your writing first.".into());
    }
    let samples = vec![text.to_string()];
    let mut profile = analyze_samples(
        format!("voice-{}", chrono_like_timestamp()),
        name.unwrap_or_else(|| "My voice".to_string()),
        VoiceContext::General,
        &samples,
    );
    if !profile.is_usable() {
        return Err(
            "That sample is too short to learn a reliable voice. Add at least 300 words.".into(),
        );
    }
    profile.strength = VoiceStrength::Balanced;
    let _ = learn_from_feedback(&mut profile);
    save(&profile)?;
    Ok(profile)
}

pub fn guidance() -> Option<String> {
    load()
        .ok()
        .flatten()
        .filter(|profile| profile.is_usable())
        .and_then(|profile| voice_prompt_guidance(&profile))
}

fn chrono_like_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_samples_are_rejected() {
        let error = learn("I write a little.", None).unwrap_err();
        assert!(error.contains("too short"));
    }
}
