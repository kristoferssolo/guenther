use std::{env, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use teloxide::types::Message;
use tokio::fs;

const MAX_INLINE_RESULTS: usize = 20;
const DEFAULT_VOICE_LINES_PATH: &str = "voice_lines.toml";
const DEFAULT_AUDIO_LINES_PATH: &str = "audio_lines.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceLine {
    pub id: String,
    pub title: String,
    pub file_id: String,
    #[serde(default)]
    pub unique_file_id: String,
    #[serde(default)]
    pub tags: Vec<String>,
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
        .filter(|line| needle.is_empty() || matches_query(line, &needle))
        .take(MAX_INLINE_RESULTS)
        .collect();

    Ok(lines)
}

#[cfg(feature = "voice-line-capture")]
pub async fn capture_incoming_voice_line(msg: &Message) -> color_eyre::Result<()> {
    let Some(capture) = capture_candidate(msg) else {
        return Ok(());
    };

    let path = match capture.target {
        CaptureTarget::Voice => voice_lines_path(),
        CaptureTarget::Audio => audio_lines_path(),
    };
    let mut file = load_voice_lines_file(&path).await?;

    if file.voice_lines.iter().any(|line| {
        line.unique_file_id == capture.line.unique_file_id || line.file_id == capture.line.file_id
    }) {
        return Ok(());
    }

    tracing::info!(
        file_id = %capture.line.file_id,
        unique_file_id = %capture.line.unique_file_id,
        target = ?capture.target,
        path = %path.display(),
        "capturing incoming voice line metadata"
    );

    file.voice_lines.push(capture.line);
    save_voice_lines_file(&path, &file).await
}

#[cfg(not(feature = "voice-line-capture"))]
pub async fn capture_incoming_voice_line(_msg: &Message) -> color_eyre::Result<()> {
    Ok(())
}

fn voice_lines_path() -> PathBuf {
    env::var("VOICE_LINES_PATH")
        .map_or_else(|_| PathBuf::from(DEFAULT_VOICE_LINES_PATH), PathBuf::from)
}

async fn load_voice_lines() -> color_eyre::Result<Vec<VoiceLine>> {
    load_voice_lines_from_paths(&[voice_lines_path(), audio_lines_path()]).await
}

fn audio_lines_path() -> PathBuf {
    env::var("AUDIO_LINES_PATH")
        .map_or_else(|_| PathBuf::from(DEFAULT_AUDIO_LINES_PATH), PathBuf::from)
}

async fn load_voice_lines_file(path: &PathBuf) -> color_eyre::Result<VoiceLinesFile> {
    match fs::read_to_string(path).await {
        Ok(content) => Ok(toml::from_str(&content)?),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(VoiceLinesFile::default()),
        Err(err) => Err(err.into()),
    }
}

async fn load_voice_lines_from_paths(paths: &[PathBuf]) -> color_eyre::Result<Vec<VoiceLine>> {
    let mut lines = Vec::new();

    for path in paths {
        let file = load_voice_lines_file(path).await?;
        lines.extend(file.voice_lines);
    }

    Ok(lines)
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
fn capture_candidate(msg: &Message) -> Option<CapturedLine> {
    if let Some(voice) = msg.voice() {
        let unique_file_id = voice.file.unique_id.to_string();
        return Some(CapturedLine {
            target: CaptureTarget::Voice,
            line: VoiceLine {
                id: unique_file_id.clone(),
                title: capture_title(msg.caption(), &unique_file_id),
                file_id: voice.file.id.to_string(),
                unique_file_id,
                tags: Vec::new(),
            },
        });
    }

    if let Some(audio) = msg.audio() {
        let unique_file_id = audio.file.unique_id.to_string();
        let title = msg
            .caption()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| audio.title.as_deref())
            .or_else(|| audio.file_name.as_deref())
            .unwrap_or("untitled audio")
            .to_owned();

        return Some(CapturedLine {
            target: CaptureTarget::Audio,
            line: VoiceLine {
                id: unique_file_id.clone(),
                title,
                file_id: audio.file.id.to_string(),
                unique_file_id,
                tags: Vec::new(),
            },
        });
    }

    None
}

#[cfg(feature = "voice-line-capture")]
fn capture_title(caption: Option<&str>, unique_file_id: &str) -> String {
    caption
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(|| format!("voice_{unique_file_id}"), ToOwned::to_owned)
}

#[cfg(feature = "voice-line-capture")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureTarget {
    Voice,
    Audio,
}

#[cfg(feature = "voice-line-capture")]
struct CapturedLine {
    target: CaptureTarget,
    line: VoiceLine,
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
    use super::{VoiceLine, load_voice_lines_from_paths, matches_query};
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
    use tokio::fs;

    fn sample_line(id: &str, title: &str, tags: &[&str]) -> VoiceLine {
        VoiceLine {
            id: id.to_owned(),
            title: title.to_owned(),
            file_id: format!("file-{id}"),
            unique_file_id: format!("unique-{id}"),
            tags: tags.iter().map(ToString::to_string).collect(),
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

    #[tokio::test]
    async fn loads_voice_and_audio_lines_for_inline_search() {
        let test_dir = temp_test_dir("inline_audio_search");
        fs::create_dir_all(&test_dir)
            .await
            .expect("create temp dir");

        let voice_path = test_dir.join("voice_lines.toml");
        let audio_path = test_dir.join("audio_lines.toml");

        fs::write(
            &voice_path,
            r#"
[[voice_lines]]
id = "voice_1"
title = "Box box"
file_id = "file-voice-1"
unique_file_id = "unique-voice-1"
tags = []
"#,
        )
        .await
        .expect("write voice lines");

        fs::write(
            &audio_path,
            r#"
[[voice_lines]]
id = "audio_1"
title = "Super Max"
file_id = "file-audio-1"
unique_file_id = "unique-audio-1"
tags = []
"#,
        )
        .await
        .expect("write audio lines");

        let lines = load_voice_lines_from_paths(&[voice_path, audio_path])
            .await
            .expect("load lines");

        assert_eq!(lines.len(), 2);
        assert!(lines.iter().any(|line| line.id == "voice_1"));
        assert!(lines.iter().any(|line| line.id == "audio_1"));
    }

    fn temp_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!("guenther_{name}_{nanos}"))
    }
}
