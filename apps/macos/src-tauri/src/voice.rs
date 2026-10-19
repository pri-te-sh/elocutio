//! Local voice profile storage. Only the aggregate style card is written; raw
//! uploaded samples are discarded after analysis.

use std::path::PathBuf;

use elocutio_voice::{
    VoiceContext, VoiceProfile, VoiceStrength, analyze_samples, voice_prompt_guidance,
};

use crate::config::Config;

fn path() -> Result<PathBuf, String> {
    Config::folder_path()
        .map(|folder| folder.join("voice-profile.json"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
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
    let path = path()?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not clear the voice profile: {error}")),
    }
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
