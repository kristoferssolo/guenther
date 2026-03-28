use super::{
    VoiceLine, VoiceLinesFile, load_voice_lines_file, save_voice_lines_file, voice_lines_path,
};
use color_eyre::eyre::{Context, eyre};
use std::{env, path::Path};
use teloxide::{
    net::Download,
    prelude::*,
    types::{FileId, InputFile, Message},
};
use tempfile::tempdir;
use tokio::{fs, process::Command};

const DEFAULT_FFMPEG_BIN: &str = "ffmpeg";

pub async fn capture_incoming_voice_line(bot: &Bot, msg: &Message) -> color_eyre::Result<()> {
    let Some(capture) = capture_candidate(msg) else {
        return Ok(());
    };

    let path = voice_lines_path();
    let mut file = load_voice_lines_file(&path).await?;

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

            let line = convert_audio_to_voice_line(bot, msg.chat.id, audio)
                .await
                .wrap_err("convert incoming audio to a reusable voice message")?;

            if contains_line(&file, &line) {
                return Ok(());
            }

            line
        }
    };

    tracing::info!(
        file_id = %line.file_id,
        unique_file_id = %line.unique_file_id,
        source_unique_file_id = %line.source_unique_file_id,
        path = %path.display(),
        "capturing incoming voice line metadata"
    );

    file.voice_lines.push(line);
    save_voice_lines_file(&path, &file).await
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
            unique_file_id,
            tags: Vec::new(),
            source_file_id: String::new(),
            source_unique_file_id: String::new(),
        }));
    }

    msg.audio().map(|audio| {
        CapturedLine::Audio(AudioCapture {
            file_id: audio.file.id.clone(),
            unique_file_id: audio.file.unique_id.to_string(),
            title: capture_audio_title(msg),
        })
    })
}

fn capture_voice_title(caption: Option<&str>, unique_file_id: &str) -> String {
    caption
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(|| format!("voice_{unique_file_id}"), ToOwned::to_owned)
}

fn capture_audio_title(msg: &Message) -> String {
    pick_audio_title(
        msg.caption(),
        msg.audio().and_then(|audio| audio.title.as_deref()),
        msg.audio().and_then(|audio| audio.file_name.as_deref()),
    )
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
        line.unique_file_id == candidate.unique_file_id || line.file_id == candidate.file_id
    })
}

fn contains_audio_source(file: &VoiceLinesFile, audio: &AudioCapture) -> bool {
    file.voice_lines.iter().any(|line| {
        line.source_unique_file_id == audio.unique_file_id
            || (!line.source_file_id.is_empty() && line.source_file_id == audio.file_id.0)
    })
}

async fn convert_audio_to_voice_line(
    bot: &Bot,
    chat_id: ChatId,
    audio: AudioCapture,
) -> color_eyre::Result<VoiceLine> {
    let source_file = bot
        .get_file(audio.file_id.clone())
        .await
        .wrap_err("fetch Telegram file metadata for incoming audio")?;

    let tempdir = tempdir().wrap_err("create temporary directory for voice conversion")?;
    let input_path = tempdir.path().join("input");
    let output_path = tempdir.path().join("voice.ogg");

    let mut input_file = fs::File::create(&input_path)
        .await
        .wrap_err("create temporary input file for downloaded audio")?;
    bot.download_file(&source_file.path, &mut input_file)
        .await
        .wrap_err("download incoming audio from Telegram")?;
    drop(input_file);

    transcode_audio_to_voice(&input_path, &output_path).await?;

    let sent_message = bot
        .send_voice(chat_id, InputFile::file(output_path))
        .await
        .wrap_err("upload converted voice message to Telegram")?;

    let voice = sent_message
        .voice()
        .cloned()
        .ok_or_else(|| eyre!("Telegram did not return a voice message after send_voice"))?;
    let unique_file_id = voice.file.unique_id.to_string();

    Ok(VoiceLine {
        id: unique_file_id.clone(),
        title: audio.title,
        file_id: voice.file.id.to_string(),
        unique_file_id,
        tags: Vec::new(),
        source_file_id: audio.file_id.0,
        source_unique_file_id: audio.unique_file_id,
    })
}

async fn transcode_audio_to_voice(input_path: &Path, output_path: &Path) -> color_eyre::Result<()> {
    let ffmpeg = env::var("FFMPEG_BIN").unwrap_or_else(|_| DEFAULT_FFMPEG_BIN.to_owned());
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
        .wrap_err("run ffmpeg to convert audio into Telegram voice format")?;

    if output.status.success() {
        return Ok(());
    }

    Err(eyre!(
        "ffmpeg failed to convert audio: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::pick_audio_title;

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
}
