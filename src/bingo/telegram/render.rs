use crate::bingo::{
    card_image::render_card_png,
    error::Result,
    model::{Card, GRID_SIDE, Position},
    store::BingoStore,
    telegram::callback::format_callback,
};
use teloxide::{
    payloads::SendMessageSetters,
    prelude::{Bot, ChatId, Requester},
    types::{InlineKeyboardButton, InlineKeyboardMarkup, InputFile, MessageId},
};

pub const HELP: &str = r"F1 bingo commands

Everyone:
/bingo games
/bingo entries [game]
/bingo get [game] [@user]
/bingo add <entry>
/bingo add <game> | <entry>

Chat administrators:
/bingo game create <slug> <name>
/bingo game activate <slug>
/bingo game close <slug>
/bingo game default <slug>
/bingo game center <slug> <text>
/bingo game description <slug> [text]
/bingo entries import <game> (attach a UTF-8 text file)
/bingo edit <entry_id> <text>
/bingo delete <entry_id>
/bingo generate [game] @user
/bingo regenerate [game] @user
/bingo reset [game] @user
/bingo card set <game> [@user] <A1-E5> <entry_id>

You can omit @user when replying to that user 's message.
Use /bingo import or /bingo reimport with a five-row, pipe-separated entry-ID grid.";

// Telegram allows 4096 characters; a smaller byte budget leaves conservative headroom.
const TELEGRAM_MESSAGE_BUDGET: usize = 3_800;

pub async fn send_games(bot: &Bot, chat_id: ChatId, store: &BingoStore) -> Result<()> {
    let games = store.list_games(chat_id).await?;
    let text = if games.is_empty() {
        "No bingo games have been created in this chat.".to_owned()
    } else {
        let mut lines = vec!["Bingo games:".to_owned()];
        lines.extend(games.into_iter().map(|game| {
            let default = if game.is_default { " · default" } else { "" };
            format!("{} – {} [{}{}]", game.slug, game.name, game.state, default)
        }));
        lines.join("\n")
    };
    send_text(bot, chat_id, &text).await
}

pub async fn send_entries(
    bot: &Bot,
    chat_id: ChatId,
    store: &BingoStore,
    slug: Option<String>,
) -> Result<()> {
    let (game, entries) = store.list_entries(chat_id, slug.as_deref()).await?;
    let mut lines = vec![format!("Entries for {} ({}):\n", game.name, entries.len())];
    lines.extend(
        entries
            .into_iter()
            .map(|entry| format!("#{} – {}", entry.id, entry.text)),
    );
    send_lines(bot, chat_id, lines).await
}

pub async fn send_card(bot: &Bot, chat_id: ChatId, card: &Card) -> Result<()> {
    let photo = bot
        .send_photo(
            chat_id,
            InputFile::memory(render_card_png(card)?).file_name("bingo-card.png"),
        )
        .await?;
    let text_result = bot
        .send_message(chat_id, render_card(card))
        .reply_markup(card_keyboard(card, Some(photo.id)))
        .await;
    if let Err(error) = text_result {
        let _ = bot.delete_message(chat_id, photo.id).await;
        return Err(error.into());
    }
    Ok(())
}

pub fn render_card(card: &Card) -> String {
    let mut lines = vec![format!("🏁 {}\n", card.game.name)];
    if !card.game.description.is_empty() {
        lines.push(card.game.description.clone());
    }
    lines.extend([
        format!("\nCard for {} · game: {}", card.owner, card.game.slug),
        String::new(),
    ]);
    lines.extend(card.cells.iter().map(|cell| {
        let marker = if cell.is_free {
            "★"
        } else if cell.marked {
            "✓"
        } else {
            "○"
        };
        format!("{marker} {}  {}", cell.position, cell.text)
    }));
    if card.has_bingo() {
        lines.push(String::new());
        lines.push("🏆 BINGO – one or more lines completed!".to_owned());
    }
    lines.join("\n")
}

pub fn card_keyboard(card: &Card, image_message_id: Option<MessageId>) -> InlineKeyboardMarkup {
    let rows = card
        .cells
        .chunks(GRID_SIDE)
        .map(|cells| {
            cells
                .iter()
                .map(|cell| {
                    let label = if cell.is_free {
                        format!("★ {}", Position::FREE)
                    } else if cell.marked {
                        format!("✓ {}", cell.position)
                    } else {
                        cell.position.to_string()
                    };
                    InlineKeyboardButton::callback(
                        label,
                        format_callback(card.id, image_message_id, cell.position),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    InlineKeyboardMarkup::new(rows)
}

pub async fn send_text(bot: &Bot, chat_id: ChatId, text: &str) -> Result<()> {
    bot.send_message(chat_id, text).await?;
    Ok(())
}

async fn send_lines(bot: &Bot, chat_id: ChatId, lines: Vec<String>) -> Result<()> {
    let mut chunk = String::new();
    for line in lines {
        if !chunk.is_empty()
            && chunk.len().saturating_add(line.len()).saturating_add(1) > TELEGRAM_MESSAGE_BUDGET
        {
            send_text(bot, chat_id, &chunk).await?;
            chunk.clear();
        }
        if !chunk.is_empty() {
            chunk.push('\n');
        }
        chunk.push_str(&line);
    }
    if !chunk.is_empty() {
        send_text(bot, chat_id, &chunk).await?;
    }
    Ok(())
}
