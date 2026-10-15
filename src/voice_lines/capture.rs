use crate::voice_lines::{
    Result, VoiceLine, VoiceLines, VoiceLinesError, VoiceLinesFile, load_voice_lines_file,
    save_voice_lines_file,
};
use std::path::Path;
use teloxide::{
    net::Download,
    prelude::*,
    types::{FileId, InputFile, Message},
};
use tempfile::tempdir;
use tokio::{fs, process::Command};

impl VoiceLines {
    /// Record an incoming voice or audio message as a new voice line.
    ///
    /// # Errors
    ///
    /// Returns an error if the voice lines file cannot be read or written, or if converting an
    /// audio message through Telegram and ffmpeg fails.
    pub async fn capture(&self, bot: &Bot, msg: &Message) -> Result<()> {
        let Some(capture) = capture_candidate(msg) else {
            return Ok(());
        };

        let mut file = load_voice_lines_file(&self.path).await?;

        let line = match capture {
            CapturedLine::Voice(line) => {
                if contains_line(&file, &line) {
                    return Ok(());
                }
                line
            }
            CapturedLine::Audio(audio) => {
                if contains_audio_source(&file, &audio) {
                    return Ok(());
                }

                let line =
                    convert_audio_to_voice_line(bot, msg.chat.id, audio, &self.ffmpeg).await?;

                if contains_line(&file, &line) {
                    return Ok(());
                }

                line
            }
        };

        tracing::info!(
            file_id = %line.file_id,
            unique_file_id = line.unique_file_id.as_deref(),
            source_unique_file_id = line.source_unique_file_id.as_deref(),
            path = %self.path.display(),
            "capturing incoming voice line metadata"
        );

        file.voice_lines.push(line);
        save_voice_lines_file(&self.path, &file).await
    }
}

enum CapturedLine {
    Voice(VoiceLine),
    Audio(AudioCapture),
}

struct AudioCapture {
    file_id: FileId,
    unique_file_id: String,
    title: String,
}

fn capture_candidate(msg: &Message) -> Option<CapturedLine> {
    if let Some(voice) = msg.voice() {
        let unique_file_id = voice.file.unique_id.to_string();
        return Some(CapturedLine::Voice(VoiceLine {
            id: unique_file_id.clone(),
            title: capture_voice_title(msg.caption(), &unique_file_id),
            file_id: voice.file.id.to_string(),
            unique_file_id: Some(unique_file_id),
            tags: Vec::new(),
            source_file_id: None,
            source_unique_file_id: None,
        }));
    }

    msg.audio().map(|audio| {
        CapturedLine::Audio(AudioCapture {
            file_id: audio.file.id.clone(),
            unique_file_id: audio.file.unique_id.to_string(),
            title: pick_audio_title(
                msg.caption(),
                audio.title.as_deref(),
                audio.file_name.as_deref(),
            ),
        })
    })
}

fn capture_voice_title(caption: Option<&str>, unique_file_id: &str) -> String {
    caption
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(|| format!("voice_{unique_file_id}"), ToOwned::to_owned)
}

fn pick_audio_title(caption: Option<&str>, title: Option<&str>, file_name: Option<&str>) -> String {
    caption
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| title.map(str::trim).filter(|value| !value.is_empty()))
        .or_else(|| file_name.map(str::trim).filter(|value| !value.is_empty()))
        .unwrap_or("untitled audio")
        .to_owned()
}

fn contains_line(file: &VoiceLinesFile, candidate: &VoiceLine) -> bool {
    file.voice_lines.iter().any(|line| {
        (line.unique_file_id.is_some() && line.unique_file_id == candidate.unique_file_id)
            || line.file_id == candidate.file_id
    })
}

fn contains_audio_source(file: &VoiceLinesFile, audio: &AudioCapture) -> bool {
    file.voice_lines.iter().any(|line| {
        line.source_unique_file_id.as_deref() == Some(audio.unique_file_id.as_str())
            || line.source_file_id.as_deref() == Some(audio.file_id.0.as_str())
    })
}

async fn convert_audio_to_voice_line(
    bot: &Bot,
    chat_id: ChatId,
    audio: AudioCapture,
    ffmpeg: &Path,
) -> Result<VoiceLine> {
    let source_file = bot
        .get_file(audio.file_id.clone())
        .await
        .map_err(VoiceLinesError::FetchAudioFile)?;

    let tempdir = tempdir().map_err(VoiceLinesError::PrepareAudio)?;
    let input_path = tempdir.path().join("input");
    let output_path = tempdir.path().join("voice.ogg");

    let mut input_file = fs::File::create(&input_path)
        .await
        .map_err(VoiceLinesError::PrepareAudio)?;
    bot.download_file(&source_file.path, &mut input_file)
        .await?;
    drop(input_file);

    transcode_audio_to_voice(ffmpeg, &input_path, &output_path).await?;

    let sent_message = bot
        .send_voice(chat_id, InputFile::file(output_path))
        .await
        .map_err(VoiceLinesError::UploadVoice)?;

    let voice = sent_message
        .voice()
        .cloned()
        .ok_or(VoiceLinesError::MissingVoice)?;
    let unique_file_id = voice.file.unique_id.to_string();

    Ok(VoiceLine {
        id: unique_file_id.clone(),
        title: audio.title,
        file_id: voice.file.id.to_string(),
        unique_file_id: Some(unique_file_id),
        tags: Vec::new(),
        source_file_id: Some(audio.file_id.0),
        source_unique_file_id: Some(audio.unique_file_id),
    })
}

async fn transcode_audio_to_voice(
    ffmpeg: &Path,
    input_path: &Path,
    output_path: &Path,
) -> Result<()> {
    let output = Command::new(ffmpeg)
        .arg("-y")
        .arg("-i")
        .arg(input_path)
        .arg("-vn")
        .arg("-c:a")
        .arg("libopus")
        .arg("-b:a")
        .arg("48k")
        .arg(output_path)
        .output()
        .await
        .map_err(VoiceLinesError::RunFfmpeg)?;

    if output.status.success() {
        return Ok(());
    }

    Err(VoiceLinesError::Ffmpeg(
        String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_audio_title_prefers_caption() {
        assert_eq!(
            pick_audio_title(
                Some("  Box box  "),
                Some("Fallback title"),
                Some("fallback.mp3")
            ),
            "Box box"
        );
    }

    #[test]
    fn pick_audio_title_falls_back_to_metadata() {
        assert_eq!(
            pick_audio_title(Some("   "), Some("Fallback title"), Some("fallback.mp3")),
            "Fallback title"
        );
    }

    #[test]
    fn contains_audio_source_ignores_lines_without_source_ids() {
        let file = VoiceLinesFile {
            voice_lines: vec![VoiceLine {
                id: "line_1".to_owned(),
                title: "Box box".to_owned(),
                file_id: "voice-file".to_owned(),
                unique_file_id: Some("voice-unique".to_owned()),
                tags: Vec::new(),
                source_file_id: None,
                source_unique_file_id: None,
            }],
        };
        let audio = AudioCapture {
            file_id: FileId("audio-file".to_owned()),
            unique_file_id: "audio-unique".to_owned(),
            title: "Box box".to_owned(),
        };

        assert!(!contains_audio_source(&file, &audio));
    }
}
