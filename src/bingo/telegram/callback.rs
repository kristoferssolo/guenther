use crate::bingo::{
    error::Result,
    model::{Position, ToggleResult},
    store::BingoStore,
    telegram::render::{card_keyboard, render_card},
};
use teloxide::{
    payloads::{AnswerCallbackQuerySetters, EditMessageTextSetters},
    prelude::{Bot, Requester},
    types::CallbackQuery,
};

pub async fn answer_callback(bot: &Bot, query: &CallbackQuery, store: &BingoStore) -> Result<()> {
    let Some(data) = query.data.as_deref() else {
        return Ok(());
    };
    let Some((card_id, position)) = parse_callback(data) else {
        return Ok(());
    };
    let result = store.toggle_cell(card_id, query.from.id, position).await;
    match result {
        Ok(toggle) => finish_toggle(bot, query, toggle).await,
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

async fn finish_toggle(bot: &Bot, query: &CallbackQuery, toggle: ToggleResult) -> Result<()> {
    bot.answer_callback_query(query.id.clone()).await?;
    if let Some(message) = query.regular_message() {
        bot.edit_message_text(message.chat.id, message.id, render_card(&toggle.card))
            .reply_markup(card_keyboard(&toggle.card))
            .await?;
        if toggle.newly_completed {
            bot.send_message(
                message.chat.id,
                format!(
                    "🏁 BINGO! {} completed a line on {}.",
                    toggle.card.owner, toggle.card.game.name
                ),
            )
            .await?;
        }
    }
    Ok(())
}

fn parse_callback(data: &str) -> Option<(i64, Position)> {
    let (card_id, position) = data.strip_prefix("b:")?.split_once(':')?;
    let card_id = card_id.parse().ok()?;
    let position = position.parse::<usize>().ok()?.try_into().ok()?;
    Some((card_id, position))
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_some_eq};

    #[test]
    fn callback_data_round_trips() {
        assert_some_eq!(
            parse_callback("b:42:7"),
            (
                42,
                Position::try_from(7_usize).expect("callback test position is valid"),
            )
        );
        assert_none!(parse_callback("other:42:7"));
        assert_none!(parse_callback("b:42"));
        assert_none!(parse_callback("b:42:25"));
    }
}
