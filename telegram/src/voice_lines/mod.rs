use serde::{Deserialize, Serialize};
use std::{env, io, path::PathBuf};
use tokio::fs;

const MAX_INLINE_RESULTS: usize = 25;
const DEFAULT_VOICE_LINES_PATH: &str = "voice_lines.toml";

#[cfg(feature = "voice-line-capture")]
mod capture;

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
    pub source_file_id: String,
    #[serde(default)]
    pub source_unique_file_id: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct VoiceLinesFile {
    #[serde(default)]
    voice_lines: Vec<VoiceLine>,
}

#[cfg(feature = "voice-line-capture")]
pub use capture::capture_incoming_voice_line;

pub async fn search_voice_lines(query: &str) -> color_eyre::Result<Vec<VoiceLine>> {
    let voice_lines = load_voice_lines().await?;
    let needle = normalize(query);

    let lines = voice_lines
        .into_iter()
        .filter(|line| needle.is_empty() || matches_query(line, &needle))
        .take(MAX_INLINE_RESULTS)
        .collect();

    Ok(lines)
}

#[cfg(not(feature = "voice-line-capture"))]
pub async fn capture_incoming_voice_line(
    _bot: &teloxide::Bot,
    _msg: &teloxide::types::Message,
) -> color_eyre::Result<()> {
    Ok(())
}

fn voice_lines_path() -> PathBuf {
    env::var("VOICE_LINES_PATH").map_or_else(|_| DEFAULT_VOICE_LINES_PATH.into(), Into::into)
}

async fn load_voice_lines() -> color_eyre::Result<Vec<VoiceLine>> {
    let file = load_voice_lines_file(&voice_lines_path()).await?;
    Ok(file.voice_lines)
}

async fn load_voice_lines_file(path: &PathBuf) -> color_eyre::Result<VoiceLinesFile> {
    match fs::read_to_string(path).await {
        Ok(content) => Ok(toml::from_str(&content)?),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(VoiceLinesFile::default()),
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
    use super::{VoiceLine, matches_query};

    fn sample_line(id: &str, title: &str, tags: &[&str]) -> VoiceLine {
        VoiceLine {
            id: id.to_owned(),
            title: title.to_owned(),
            file_id: format!("file-{id}"),
            unique_file_id: format!("unique-{id}"),
            tags: tags.iter().map(ToString::to_string).collect(),
            source_file_id: String::new(),
            source_unique_file_id: String::new(),
        }
    }

    #[test]
    fn matches_by_title() {
        let line = sample_line("line_1", "This is not acceptable", &["angry"]);
        assert!(matches_query(&line, "acceptable"));
    }

    #[test]
    fn matches_by_tag() {
        let line = sample_line("line_2", "We look like amateurs", &["team", "mess"]);
        assert!(matches_query(&line, "mess"));
    }

    #[test]
    fn ignores_unknown_fields_in_toml() {
        let parsed = toml::from_str::<VoiceLine>(
            r#"
id = "line_1"
title = "Sample"
file_id = "file-1"
unique_file_id = "unique-1"
tags = []
kind = "voice"
"#,
        )
        .expect("parse voice line");

        assert_eq!(parsed.id, "line_1");
    }
}
