use crate::handler::Result;
use crate::{
    cache::CachedMedia,
    comments::TELEGRAM_CAPTION_LIMIT,
    config::Platform,
    utils::{MediaKind, truncate_with_ellipsis},
};
use std::path::PathBuf;
use teloxide::{
    Bot,
    prelude::*,
    types::{ChatId, InputFile, InputMedia, InputMediaPhoto, InputMediaVideo},
};
use tracing::info;

/// Maximum number of items Telegram accepts in a single media group.
const MEDIA_GROUP_LIMIT: usize = 10;

/// Send media, grouping multiple items into a single album message.
///
/// Only the first item of each message carries the caption, so Telegram shows
/// it once for the whole album. Returns the sent media mapped back to cache
/// entries; items that Telegram did not echo as a photo/video are skipped.
pub(super) async fn send_media(
    bot: &Bot,
    chat_id: ChatId,
    mut media_items: Vec<(InputFile, MediaKind)>,
    caption: &str,
) -> Result<Vec<CachedMedia>> {
    if media_items.len() == 1
        && let Some((input, kind)) = media_items.pop()
    {
        let message = send_single(bot, chat_id, input, kind, caption).await?;
        return Ok(extract_cached_media(&message, kind).into_iter().collect());
    }

    let mut cached = Vec::new();
    while !media_items.is_empty() {
        let chunk_len = media_items.len().min(MEDIA_GROUP_LIMIT);
        let chunk = media_items.drain(..chunk_len).collect::<Vec<_>>();
        let group = chunk
            .iter()
            .enumerate()
            .map(|(index, (input, kind))| {
                let caption = (index == 0).then(|| caption.to_owned());
                into_input_media(input.clone(), *kind, caption)
            })
            .collect::<Vec<_>>();

        let messages = bot.send_media_group(chat_id, group).await?;
        info!(media_count = messages.len(), "Album sent");
        cached.extend(
            chunk
                .iter()
                .zip(&messages)
                .filter_map(|((_, kind), message)| extract_cached_media(message, *kind)),
        );
    }

    Ok(cached)
}

fn into_input_media(input: InputFile, kind: MediaKind, caption: Option<String>) -> InputMedia {
    match kind {
        MediaKind::Video => InputMedia::Video(InputMediaVideo {
            caption,
            ..InputMediaVideo::new(input)
        }),
        MediaKind::Image => InputMedia::Photo(InputMediaPhoto {
            caption,
            ..InputMediaPhoto::new(input)
        }),
    }
}

/// Map a sent message back to a cacheable media item, or `None` when the
/// message carries no matching photo/video.
fn extract_cached_media(message: &Message, kind: MediaKind) -> Option<CachedMedia> {
    match kind {
        MediaKind::Video => message.video().map(|video| CachedMedia {
            kind: MediaKind::Video,
            file_id: video.file.id.0.clone(),
        }),
        MediaKind::Image => {
            let photo = message
                .photo()?
                .iter()
                .max_by_key(|size| (size.width, size.height))?;
            Some(CachedMedia {
                kind: MediaKind::Image,
                file_id: photo.file.id.0.clone(),
            })
        }
    }
}

async fn send_single(
    bot: &Bot,
    chat_id: ChatId,
    input: InputFile,
    kind: MediaKind,
    caption: &str,
) -> Result<Message> {
    let message = match kind {
        MediaKind::Video => bot.send_video(chat_id, input).caption(caption).await?,
        MediaKind::Image => bot.send_photo(chat_id, input).caption(caption).await?,
    };
    info!(message_id = message.id.to_string(), "{} sent", kind);
    Ok(message)
}

pub(super) fn should_include_source_text(
    platform: Platform,
    media_items: &[(PathBuf, MediaKind)],
    source_text: Option<&str>,
) -> bool {
    matches!(platform, Platform::Twitter)
        && source_text.is_some()
        && media_items
            .iter()
            .all(|(_, kind)| matches!(kind, MediaKind::Image))
}

pub(super) fn compose_caption(quote: &str, source_text: Option<&str>) -> String {
    let Some(source_text) = source_text.map(str::trim).filter(|text| !text.is_empty()) else {
        return quote.to_owned();
    };

    let combined = format!("{quote}\n\n{source_text}");
    if combined.chars().count() <= TELEGRAM_CAPTION_LIMIT {
        return combined;
    }

    let reserved = quote.chars().count().saturating_add(2);
    if reserved >= TELEGRAM_CAPTION_LIMIT {
        return quote.to_owned();
    }

    let available = TELEGRAM_CAPTION_LIMIT.saturating_sub(reserved);
    let truncated = truncate_with_ellipsis(source_text, available);
    format!("{quote}\n\n{truncated}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_some, assert_some_eq};

    #[test]
    fn compose_caption_appends_source_text() {
        let caption = compose_caption("quote", Some("tweet text"));
        assert_eq!(caption, "quote\n\ntweet text");
    }

    #[test]
    fn input_media_carries_only_the_given_caption() {
        let photo = into_input_media(
            InputFile::file(PathBuf::from("photo.jpg")),
            MediaKind::Image,
            Some("caption".to_owned()),
        );
        let photo = assert_some!(match photo {
            InputMedia::Photo(photo) => Some(photo),
            _ => None,
        });
        assert_some_eq!(photo.caption, "caption");

        let video = into_input_media(
            InputFile::file(PathBuf::from("video.mp4")),
            MediaKind::Video,
            None,
        );
        let video = assert_some!(match video {
            InputMedia::Video(video) => Some(video),
            _ => None,
        });
        assert_none!(video.caption);
    }

    #[test]
    fn compose_caption_ignores_missing_source_text() {
        let caption = compose_caption("quote", None);
        assert_eq!(caption, "quote");
    }
}
