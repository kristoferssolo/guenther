use serde::{Deserialize, Serialize};
#[cfg(feature = "voice-line-capture")]
use std::io::Write;
use std::{
    env, io,
    path::{Path, PathBuf},
    sync::Arc,
};
#[cfg(feature = "voice-line-capture")]
use tempfile::NamedTempFile;
use tokio::fs;
#[cfg(feature = "voice-line-capture")]
use tokio::task;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unique_file_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_file_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_unique_file_id: Option<String>,
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
        let needle = query.trim();

        let lines = voice_lines
            .into_iter()
            .filter(|line| needle.is_empty() || matches_query(line, needle))
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

/// Writes to a temp file and renames it over `path`, so a crash can't leave a truncated file.
#[cfg(feature = "voice-line-capture")]
async fn save_voice_lines_file(path: &Path, file: &VoiceLinesFile) -> color_eyre::Result<()> {
    let content = toml::to_string_pretty(file)?;
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_owned();
    fs::create_dir_all(&directory).await?;

    let path = path.to_owned();
    task::spawn_blocking(move || -> color_eyre::Result<()> {
        let mut temp = NamedTempFile::new_in(&directory)?;
        temp.write_all(content.as_bytes())?;
        temp.as_file().sync_all()?;
        temp.persist(&path)?;
        Ok(())
    })
    .await?
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

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_ok};
    use rstest::rstest;

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
            unique_file_id: Some(format!("unique-{id}")),
            tags: tags.iter().map(ToString::to_string).collect(),
            source_file_id: None,
            source_unique_file_id: None,
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

    #[test]
    fn missing_file_ids_load_as_none_and_are_not_serialized() {
        let parsed = assert_ok!(toml::from_str::<VoiceLine>(
            r#"
id = "line_1"
title = "Sample"
file_id = "file-1"
"#,
        ));

        assert_none!(&parsed.unique_file_id);
        assert_none!(&parsed.source_file_id);
        assert_none!(&parsed.source_unique_file_id);

        let serialized = assert_ok!(toml::to_string(&parsed));
        assert!(!serialized.contains("unique_file_id"));
        assert!(!serialized.contains("source_file_id"));
    }

    #[rstest]
    #[case("pit")]
    #[case("PIT")]
    #[tokio::test]
    async fn search_reads_from_the_bound_path(#[case] query: &str) {
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

        let matches = assert_ok!(voice_lines.search(query).await);

        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches.first().map(|line| line.id.as_str()),
            Some("box_box")
        );
    }

    #[cfg(feature = "voice-line-capture")]
    #[tokio::test]
    async fn save_round_trips_without_leaving_temp_files() {
        let directory = assert_ok!(tempfile::tempdir());
        let path = directory.path().join("voice_lines.toml");
        let file = VoiceLinesFile {
            voice_lines: vec![sample_line("box_box", "Box box", &["pit"])],
        };

        assert_ok!(save_voice_lines_file(&path, &file).await);
        let loaded = assert_ok!(load_voice_lines_file(&path).await);

        assert_eq!(loaded.voice_lines, file.voice_lines);
        let entries = assert_ok!(
            assert_ok!(std::fs::read_dir(directory.path()))
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<io::Result<Vec<_>>>()
        );
        assert_eq!(entries, [path]);
    }
}
