use crate::handler::{HandlerError, Result};
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
    types::{ChatId, InputFile, InputMedia, InputMediaPhoto, InputMediaVideo, MessageId},
};
use tracing::debug;

/// Maximum number of items Telegram accepts in a single media group.
const MEDIA_GROUP_LIMIT: usize = 10;

/// Media that reached the chat.
#[derive(Debug, Default)]
pub(super) struct SentMedia {
    /// Sent items mapped back to cache entries; items that Telegram did not
    /// echo as a photo/video are skipped.
    pub cached: Vec<CachedMedia>,
    /// Messages carrying the caption: the single message, or the first item
    /// of each album.
    pub captioned: Vec<MessageId>,
}

impl SentMedia {
    fn append(&mut self, other: Self) {
        self.cached.extend(other.cached);
        self.captioned.extend(other.captioned);
    }
}

/// What [`send_media`] delivered before Telegram rejected a message, if it
/// did.
#[derive(Debug)]
pub(super) struct Delivery {
    pub sent: SentMedia,
    pub failure: Option<DeliveryFailure>,
}

/// A rejected message, which stops the delivery.
#[derive(Debug)]
pub(super) struct DeliveryFailure {
    /// Items in the rejected message and in every message after it.
    pub unsent: usize,
    pub error: HandlerError,
}

impl Delivery {
    /// The sent media, or the error if anything was left unsent.
    pub(super) fn complete(self) -> Result<SentMedia> {
        match self.failure {
            Some(failure) => Err(failure.error),
            None => Ok(self.sent),
        }
    }
}

/// Send media, grouping multiple items into a single album message.
///
/// Only the first item of each message carries the caption, so Telegram shows
/// it once for the whole album. Sending stops at the first rejected message;
/// the albums before it stay in the chat and in the result.
pub(super) async fn send_media(
    bot: &Bot,
    chat_id: ChatId,
    mut media_items: Vec<(InputFile, MediaKind)>,
    caption: &str,
) -> Delivery {
    if media_items.len() == 1
        && let Some((input, kind)) = media_items.pop()
    {
        return match send_single(bot, chat_id, input, kind, caption).await {
            Ok(message) => Delivery {
                sent: SentMedia {
                    cached: extract_cached_media(&message, kind).into_iter().collect(),
                    captioned: vec![message.id],
                },
                failure: None,
            },
            Err(error) => Delivery {
                sent: SentMedia::default(),
                failure: Some(DeliveryFailure { unsent: 1, error }),
            },
        };
    }

    send_albums(media_items, |album| {
        send_album(bot, chat_id, album, caption)
    })
    .await
}

/// Send `items` as balanced albums through `send_album`, stopping at the
/// first album it fails to send.
async fn send_albums<T, F, Fut>(mut items: Vec<T>, mut send_album: F) -> Delivery
where
    F: FnMut(Vec<T>) -> Fut,
    Fut: Future<Output = Result<SentMedia>>,
{
    let mut sent = SentMedia::default();
    for album_len in album_sizes(items.len()) {
        let album = items.drain(..album_len).collect::<Vec<_>>();
        match send_album(album).await {
            Ok(album) => sent.append(album),
            Err(error) => {
                return Delivery {
                    sent,
                    failure: Some(DeliveryFailure {
                        unsent: album_len.saturating_add(items.len()),
                        error,
                    }),
                };
            }
        }
    }
    Delivery {
        sent,
        failure: None,
    }
}

async fn send_album(
    bot: &Bot,
    chat_id: ChatId,
    album: Vec<(InputFile, MediaKind)>,
    caption: &str,
) -> Result<SentMedia> {
    let kinds = album.iter().map(|(_, kind)| *kind).collect::<Vec<_>>();
    let group = album
        .into_iter()
        .enumerate()
        .map(|(index, (input, kind))| {
            let caption = (index == 0).then(|| caption.to_owned());
            into_input_media(input, kind, caption)
        })
        .collect::<Vec<_>>();

    let messages = bot.send_media_group(chat_id, group).await?;
    debug!(
        media_count = messages.len(),
        message_id = messages.first().map(|message| message.id.0),
        "Sent album"
    );
    Ok(SentMedia {
        cached: kinds
            .into_iter()
            .zip(&messages)
            .filter_map(|(kind, message)| extract_cached_media(message, kind))
            .collect(),
        captioned: messages
            .first()
            .map(|message| message.id)
            .into_iter()
            .collect(),
    })
}

/// Split `len` items into the fewest albums Telegram accepts, balancing their
/// sizes so none falls below Telegram's 2-item minimum (11 becomes 6 + 5, not
/// 10 + 1).
fn album_sizes(len: usize) -> impl Iterator<Item = usize> {
    let albums = len.div_ceil(MEDIA_GROUP_LIMIT);
    let base = len.checked_div(albums).unwrap_or_default();
    let larger = len.checked_rem(albums).unwrap_or_default();
    (0..albums).map(move |index| {
        if index < larger {
            base.saturating_add(1)
        } else {
            base
        }
    })
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
pub(super) fn extract_cached_media(message: &Message, kind: MediaKind) -> Option<CachedMedia> {
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
    debug!(message_id = message.id.0, %kind, "Sent media");
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

/// Notice for media over the upload limit that points to the source instead.
pub(super) fn too_large_notice(source_url: &str, dropped: usize, total: usize) -> String {
    if dropped < total {
        return format!("{dropped} of {total} items were too large for Telegram: {source_url}");
    }
    format!("Too large for Telegram, watch it at the source: {source_url}")
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
    use rstest::rstest;
    use std::future;
    use teloxide::{ApiError, RequestError};

    #[rstest]
    #[case(2, &[2])]
    #[case(10, &[10])]
    #[case(11, &[6, 5])]
    #[case(19, &[10, 9])]
    #[case(20, &[10, 10])]
    fn album_sizes_stay_within_telegram_limits(#[case] len: usize, #[case] expected: &[usize]) {
        assert_eq!(album_sizes(len).collect::<Vec<_>>(), expected);
    }

    /// Sends `album` as messages whose ids are the item numbers, unless it
    /// holds the `rejected` item.
    fn fake_album(album: &[i32], rejected: Option<i32>) -> Result<SentMedia> {
        if rejected.is_some_and(|item| album.contains(&item)) {
            return Err(RequestError::Api(ApiError::RequestEntityTooLarge).into());
        }
        Ok(SentMedia {
            cached: album
                .iter()
                .map(|item| CachedMedia {
                    kind: MediaKind::Image,
                    file_id: item.to_string(),
                })
                .collect(),
            captioned: album.first().copied().map(MessageId).into_iter().collect(),
        })
    }

    #[rstest]
    #[case::all_sent(12, None, &[0, 6], 12, None)]
    #[case::later_album_rejected(25, Some(9), &[0], 9, Some(16))]
    #[case::first_album_rejected(12, Some(0), &[], 0, Some(12))]
    #[tokio::test]
    async fn delivery_keeps_albums_sent_before_a_rejection(
        #[case] len: i32,
        #[case] rejected: Option<i32>,
        #[case] captioned: &[i32],
        #[case] cached: usize,
        #[case] unsent: Option<usize>,
    ) {
        let delivery = send_albums((0..len).collect(), |album| {
            future::ready(fake_album(&album, rejected))
        })
        .await;
        assert_eq!(
            delivery.sent.captioned,
            captioned.iter().copied().map(MessageId).collect::<Vec<_>>()
        );
        assert_eq!(delivery.sent.cached.len(), cached);
        assert_eq!(delivery.failure.map(|failure| failure.unsent), unsent);
    }

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

    #[rstest]
    #[case::everything(
        3,
        3,
        "Too large for Telegram, watch it at the source: https://x.com/a/status/1"
    )]
    #[case::part(
        1,
        3,
        "1 of 3 items were too large for Telegram: https://x.com/a/status/1"
    )]
    fn too_large_notice_keeps_the_source_link(
        #[case] dropped: usize,
        #[case] total: usize,
        #[case] expected: &str,
    ) {
        assert_eq!(
            too_large_notice("https://x.com/a/status/1", dropped, total),
            expected
        );
    }

    #[test]
    fn compose_caption_ignores_missing_source_text() {
        let caption = compose_caption("quote", None);
        assert_eq!(caption, "quote");
    }
}
