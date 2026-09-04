use serde::{Deserialize, Serialize};
use std::{
    env, io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs;

const MAX_INLINE_RESULTS: usize = 25;
const DEFAULT_VOICE_LINES_PATH: &str = "voice_lines.toml";
#[cfg(feature = "voice-line-capture")]
const DEFAULT_FFMPEG_BIN: &str = "ffmpeg";

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

/// Searches and optionally captures voice lines using startup-bound paths.
#[derive(Debug, Clone)]
pub struct VoiceLines {
    path: Arc<Path>,
    #[cfg(feature = "voice-line-capture")]
    ffmpeg: Arc<Path>,
}

impl VoiceLines {
    pub fn from_env() -> Self {
        Self {
            path: env_path("VOICE_LINES_PATH", DEFAULT_VOICE_LINES_PATH),
            #[cfg(feature = "voice-line-capture")]
            ffmpeg: env_path("FFMPEG_BIN", DEFAULT_FFMPEG_BIN),
        }
    }

    pub async fn search(&self, query: &str) -> color_eyre::Result<Vec<VoiceLine>> {
        let voice_lines = load_voice_lines_file(&self.path).await?.voice_lines;
        let needle = normalize(query);

        let lines = voice_lines
            .into_iter()
            .filter(|line| needle.is_empty() || matches_query(line, &needle))
            .take(MAX_INLINE_RESULTS)
            .collect();

        Ok(lines)
    }
}

fn env_path(key: &str, default: &str) -> Arc<Path> {
    env::var_os(key)
        .map_or_else(|| PathBuf::from(default), PathBuf::from)
        .into()
}

async fn load_voice_lines_file(path: &Path) -> color_eyre::Result<VoiceLinesFile> {
    match fs::read_to_string(path).await {
        Ok(content) => Ok(toml::from_str(&content)?),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(VoiceLinesFile::default()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(feature = "voice-line-capture")]
async fn save_voice_lines_file(path: &Path, file: &VoiceLinesFile) -> color_eyre::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).await?;
    }

    fs::write(path, toml::to_string_pretty(file)?).await?;
    Ok(())
}

fn matches_query(line: &VoiceLine, needle: &str) -> bool {
    contains_ignore_ascii_case(&line.title, needle)
        || contains_ignore_ascii_case(&line.id, needle)
        || line
            .tags
            .iter()
            .any(|tag| contains_ignore_ascii_case(tag, needle))
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }

    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn normalize(text: &str) -> String {
    text.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;

    fn voice_lines_at(path: PathBuf) -> VoiceLines {
        VoiceLines {
            path: path.into(),
            #[cfg(feature = "voice-line-capture")]
            ffmpeg: PathBuf::from(DEFAULT_FFMPEG_BIN).into(),
        }
    }

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
        let parsed = assert_ok!(toml::from_str::<VoiceLine>(
            r#"
id = "line_1"
title = "Sample"
file_id = "file-1"
unique_file_id = "unique-1"
tags = []
kind = "voice"
"#,
        ));

        assert_eq!(parsed.id, "line_1");
    }

    #[tokio::test]
    async fn search_reads_from_the_bound_path() {
        let directory = assert_ok!(tempfile::tempdir());
        let path = directory.path().join("custom.toml");
        assert_ok!(std::fs::write(
            &path,
            r#"
[[voice_lines]]
id = "box_box"
title = "Box box"
file_id = "telegram-file"
tags = ["pit"]
"#,
        ));
        let voice_lines = voice_lines_at(path);

        let matches = assert_ok!(voice_lines.search("pit").await);

        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches.first().map(|line| line.id.as_str()),
            Some("box_box")
        );
    }
}
