use std::{env, path::PathBuf};

use serde::{Deserialize, Serialize};
use teloxide::types::Message;
use tokio::fs;

const MAX_INLINE_RESULTS: usize = 20;
const DEFAULT_VOICE_LINES_PATH: &str = "voice_lines.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceLine {
    pub id: String,
    pub title: String,
    pub file_id: String,
    #[serde(default)]
    pub unique_file_id: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub kind: VoiceLineKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VoiceLineKind {
    #[default]
    Voice,
    Audio,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct VoiceLinesFile {
    #[serde(default)]
    voice_lines: Vec<VoiceLine>,
}

pub async fn search_voice_lines(query: &str) -> color_eyre::Result<Vec<VoiceLine>> {
    let voice_lines = load_voice_lines().await?;
    let needle = normalize(query);

    let lines = voice_lines
        .into_iter()
        .filter(|line| line.kind == VoiceLineKind::Voice)
        .filter(|line| needle.is_empty() || matches_query(line, &needle))
        .take(MAX_INLINE_RESULTS)
        .collect();

    Ok(lines)
}

#[cfg(feature = "voice-line-capture")]
pub async fn capture_incoming_voice_line(msg: &Message) -> color_eyre::Result<()> {
    let Some(candidate) = capture_candidate(msg) else {
        return Ok(());
    };

    let path = voice_lines_path();
    let mut file = load_voice_lines_file(&path).await?;

    if file.voice_lines.iter().any(|line| {
        line.unique_file_id == candidate.unique_file_id || line.file_id == candidate.file_id
    }) {
        return Ok(());
    }

    tracing::info!(
        file_id = %candidate.file_id,
        unique_file_id = %candidate.unique_file_id,
        kind = ?candidate.kind,
        path = %path.display(),
        "capturing incoming voice line metadata"
    );

    file.voice_lines.push(candidate);
    save_voice_lines_file(&path, &file).await
}

#[cfg(not(feature = "voice-line-capture"))]
pub async fn capture_incoming_voice_line(_msg: &Message) -> color_eyre::Result<()> {
    Ok(())
}

fn voice_lines_path() -> PathBuf {
    env::var("VOICE_LINES_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_VOICE_LINES_PATH))
}

async fn load_voice_lines() -> color_eyre::Result<Vec<VoiceLine>> {
    let file = load_voice_lines_file(&voice_lines_path()).await?;
    Ok(file.voice_lines)
}

async fn load_voice_lines_file(path: &PathBuf) -> color_eyre::Result<VoiceLinesFile> {
    match fs::read_to_string(path).await {
        Ok(content) => Ok(toml::from_str(&content)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(VoiceLinesFile::default()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(feature = "voice-line-capture")]
async fn save_voice_lines_file(path: &PathBuf, file: &VoiceLinesFile) -> color_eyre::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).await?;
    }

    fs::write(path, toml::to_string_pretty(file)?).await?;
    Ok(())
}

#[cfg(feature = "voice-line-capture")]
fn capture_candidate(msg: &Message) -> Option<VoiceLine> {
    if let Some(voice) = msg.voice() {
        let unique_file_id = voice.file.unique_id.to_string();
        return Some(VoiceLine {
            id: unique_file_id.clone(),
            title: capture_title(msg.caption(), None, &unique_file_id, VoiceLineKind::Voice),
            file_id: voice.file.id.to_string(),
            unique_file_id,
            tags: Vec::new(),
            kind: VoiceLineKind::Voice,
        });
    }

    if let Some(audio) = msg.audio() {
        let unique_file_id = audio.file.unique_id.to_string();
        return Some(VoiceLine {
            id: unique_file_id.clone(),
            title: capture_title(
                msg.caption(),
                audio.title.as_deref().or(audio.file_name.as_deref()),
                &unique_file_id,
                VoiceLineKind::Audio,
            ),
            file_id: audio.file.id.to_string(),
            unique_file_id,
            tags: Vec::new(),
            kind: VoiceLineKind::Audio,
        });
    }

    None
}

#[cfg(feature = "voice-line-capture")]
fn capture_title(
    caption: Option<&str>,
    fallback: Option<&str>,
    unique_file_id: &str,
    kind: VoiceLineKind,
) -> String {
    caption
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| fallback.map(str::trim).filter(|value| !value.is_empty()))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| match kind {
            VoiceLineKind::Voice => format!("voice_{unique_file_id}"),
            VoiceLineKind::Audio => format!("audio_{unique_file_id}"),
        })
}

#[inline]
fn matches_query(line: &VoiceLine, needle: &str) -> bool {
    contains_ignore_ascii_case(&line.title, needle)
        || contains_ignore_ascii_case(&line.id, needle)
        || line
            .tags
            .iter()
            .any(|tag| contains_ignore_ascii_case(tag, needle))
}

#[inline]
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }

    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

#[inline]
fn normalize(text: &str) -> String {
    text.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{VoiceLine, VoiceLineKind, matches_query};

    fn sample_line(id: &str, title: &str, tags: &[&str], kind: VoiceLineKind) -> VoiceLine {
        VoiceLine {
            id: id.to_owned(),
            title: title.to_owned(),
            file_id: format!("file-{id}"),
            unique_file_id: format!("unique-{id}"),
            tags: tags.iter().map(ToString::to_string).collect(),
            kind,
        }
    }

    #[test]
    fn matches_by_title() {
        let line = sample_line(
            "line_1",
            "This is not acceptable",
            &["angry"],
            VoiceLineKind::Voice,
        );
        assert!(matches_query(&line, "acceptable"));
    }

    #[test]
    fn matches_by_tag() {
        let line = sample_line(
            "line_2",
            "We look like amateurs",
            &["team", "mess"],
            VoiceLineKind::Voice,
        );
        assert!(matches_query(&line, "mess"));
    }

    #[test]
    fn voice_kind_defaults_to_voice() {
        let parsed = toml::from_str::<VoiceLine>(
            r#"
id = "line_1"
title = "Sample"
file_id = "file-1"
unique_file_id = "unique-1"
tags = []
"#,
        )
        .expect("parse voice line");

        assert_eq!(parsed.kind, VoiceLineKind::Voice);
    }
}
