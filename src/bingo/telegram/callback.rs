use crate::bingo::{
    card_image::render_card_png,
    error::Result,
    model::{CardId, Position, ToggleResult},
    store::BingoStore,
    telegram::{
        admin::{AdminCache, is_user_chat_admin},
        render::{card_keyboard, render_card},
    },
};
use teloxide::{
    ApiError, RequestError,
    payloads::{AnswerCallbackQuerySetters, EditMessageTextSetters},
    prelude::{Bot, Requester},
    types::{CallbackQuery, ChatId, InputFile, InputMedia, InputMediaPhoto, MessageId},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CardCallback {
    card_id: CardId,
    image_message_id: Option<MessageId>,
    position: Position,
}

pub async fn answer_callback(
    bot: &Bot,
    query: &CallbackQuery,
    store: &BingoStore,
    admin_cache: &AdminCache,
) -> Result<()> {
    let Some(data) = query.data.as_deref() else {
        return Ok(());
    };
    let Some(callback) = parse_callback(data) else {
        return Ok(());
    };
    let can_edit_any_card = if let Some(message) = query.regular_message() {
        is_user_chat_admin(
            bot,
            message.chat.id,
            query.from.id,
            message.chat.is_private(),
            admin_cache,
        )
        .await?
    } else {
        false
    };
    let result = store
        .toggle_cell(
            callback.card_id,
            query.from.id,
            callback.position,
            can_edit_any_card,
        )
        .await;
    match result {
        Ok(toggle) => finish_toggle(bot, query, toggle, callback.image_message_id).await,
        Err(error) if error.is_user_facing() => {
            bot.answer_callback_query(query.id.clone())
                .text(error.to_string())
                .show_alert(true)
                .await?;
            Ok(())
        }
        Err(error) => {
            let _ = bot
                .answer_callback_query(query.id.clone())
                .text("Could not update the card")
                .show_alert(true)
                .await;
            Err(error)
        }
    }
}

async fn finish_toggle(
    bot: &Bot,
    query: &CallbackQuery,
    toggle: ToggleResult,
    image_message_id: Option<MessageId>,
) -> Result<()> {
    bot.answer_callback_query(query.id.clone()).await?;
    if let Some(message) = query.regular_message() {
        edit_card_image(bot, message.chat.id, image_message_id, &toggle).await?;
        accept_unchanged(
            bot.edit_message_text(message.chat.id, message.id, render_card(&toggle.card))
                .reply_markup(card_keyboard(&toggle.card, image_message_id))
                .await,
        )?;
        if toggle.newly_completed {
            bot.send_message(
                message.chat.id,
                win_message(&toggle.card.owner.to_string(), &toggle.card.game.name),
            )
            .await?;
        }
    }
    Ok(())
}

fn win_message(owner: &str, game_name: &str) -> String {
    format!(
        "🏁 FOKING BINGO! {owner} completed a line on {game_name}. Finally, somebody did their foking job."
    )
}

async fn edit_card_image(
    bot: &Bot,
    chat_id: ChatId,
    image_message_id: Option<MessageId>,
    toggle: &ToggleResult,
) -> Result<()> {
    let Some(image_message_id) = image_message_id else {
        return Ok(());
    };
    let photo = InputFile::memory(render_card_png(&toggle.card)?).file_name("bingo-card.png");
    accept_unchanged(
        bot.edit_message_media(
            chat_id,
            image_message_id,
            InputMedia::Photo(InputMediaPhoto::new(photo)),
        )
        .await,
    )?;
    Ok(())
}

fn accept_unchanged<T>(result: std::result::Result<T, RequestError>) -> Result<()> {
    match result {
        Ok(_) | Err(RequestError::Api(ApiError::MessageNotModified)) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn format_callback(
    card_id: CardId,
    image_message_id: Option<MessageId>,
    position: Position,
) -> String {
    let data = image_message_id.map_or_else(
        || format!("b:{card_id}:{}", position.index()),
        |message_id| format!("b:{card_id}:{}:{}", message_id.0, position.index()),
    );
    debug_assert!(data.len() <= 64, "Telegram callback data exceeds 64 bytes");
    data
}

fn parse_callback(data: &str) -> Option<CardCallback> {
    let parts = data.strip_prefix("b:")?.split(':').collect::<Vec<_>>();
    let (card_id, image_message_id, position) = match parts.as_slice() {
        [card_id, position] => (card_id.parse().ok()?, None, position),
        [card_id, image_message_id, position] => (
            card_id.parse().ok()?,
            Some(MessageId(image_message_id.parse().ok()?)),
            position,
        ),
        _ => return None,
    };
    Some(CardCallback {
        card_id,
        image_message_id,
        position: position.parse::<usize>().ok()?.try_into().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_none, assert_ok, assert_some_eq};

    #[test]
    fn callback_data_round_trips() {
        assert_some_eq!(
            parse_callback("b:42:7"),
            CardCallback {
                card_id: CardId::from(42),
                image_message_id: None,
                position: assert_ok!(Position::try_from(7_usize)),
            }
        );
        assert_some_eq!(
            parse_callback("b:42:314:7"),
            CardCallback {
                card_id: CardId::from(42),
                image_message_id: Some(MessageId(314)),
                position: assert_ok!(Position::try_from(7_usize)),
            }
        );
        assert_none!(parse_callback("other:42:7"));
        assert_none!(parse_callback("b:42"));
        assert_none!(parse_callback("b:42:25"));
        assert_none!(parse_callback("b:42:314:7:extra"));
    }

    #[test]
    fn callback_data_preserves_image_message_ids_and_fits_telegram() {
        let position = assert_ok!(Position::try_from(24_usize));
        let legacy = format_callback(CardId::from(i64::MAX), None, position);
        let current = format_callback(CardId::from(i64::MIN), Some(MessageId(i32::MIN)), position);
        assert_eq!(legacy, format!("b:{}:24", i64::MAX));
        assert_eq!(current, format!("b:{}:{}:24", i64::MIN, i32::MIN));
        assert!(legacy.len() <= 64);
        assert!(current.len() <= 64);
    }

    #[test]
    fn accepts_idempotent_telegram_message_edits() {
        assert_ok!(accept_unchanged::<()>(Ok(())));
        assert_ok!(accept_unchanged::<()>(Err(RequestError::Api(
            ApiError::MessageNotModified
        ))));
        assert_err!(accept_unchanged::<()>(Err(RequestError::Api(
            ApiError::BotBlocked
        ))));
    }

    #[test]
    fn renders_a_guenther_style_win_message() {
        assert_eq!(
            win_message("@driver", "2026 Season"),
            "🏁 FOKING BINGO! @driver completed a line on 2026 Season. Finally, somebody did their foking job."
        );
    }
}
