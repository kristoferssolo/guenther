use crate::bingo::{
    command::{BingoCommand, CardAdmin, EntryAdmin, GameAdmin},
    error::{BingoError, Result},
    model::{Card, GRID_SIDE, KnownUser, Position, ToggleResult},
    store::BingoStore,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use teloxide::{
    payloads::{AnswerCallbackQuerySetters, EditMessageTextSetters, SendMessageSetters},
    prelude::*,
    types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, Message, User, UserId},
};
use tokio::sync::Mutex;

const HELP: &str = r"F1 bingo commands

Everyone:
/bingo games
/bingo entries [game]
/bingo get [game] [@user]

Chat administrators:
/bingo game create <slug> <name>
/bingo game activate <slug>
/bingo game close <slug>
/bingo game default <slug>
/bingo game center <slug> <text>
/bingo add <entry>
/bingo add <game> | <entry>
/bingo edit <entry_id> <text>
/bingo delete <entry_id>
/bingo generate [game] @user
/bingo regenerate [game] @user
/bingo reset [game] @user
/bingo card set <game> [@user] <A1-E5> <text>

You can omit @user when replying to that user's message. Use /bingo import or /bingo reimport with a five-row, pipe-separated grid to migrate a manual card.";

// Telegram allows 4096 characters; a smaller byte budget leaves conservative headroom.
const TELEGRAM_MESSAGE_BUDGET: usize = 3_800;
const ADMIN_CACHE_TTL: Duration = Duration::from_mins(1);

#[derive(Debug, Clone)]
pub struct AdminCache {
    entries: Arc<Mutex<HashMap<ChatId, CachedAdministrators>>>,
    ttl: Duration,
}

#[derive(Debug)]
struct CachedAdministrators {
    expires_at: Instant,
    user_ids: HashSet<UserId>,
}

impl Default for AdminCache {
    fn default() -> Self {
        Self {
            entries: Arc::default(),
            ttl: ADMIN_CACHE_TTL,
        }
    }
}

impl AdminCache {
    async fn status(&self, chat_id: ChatId, user_id: UserId) -> Option<bool> {
        let mut entries = self.entries.lock().await;
        if entries
            .get(&chat_id)
            .is_some_and(|entry| entry.expires_at <= Instant::now())
        {
            entries.remove(&chat_id);
            return None;
        }
        entries
            .get(&chat_id)
            .map(|entry| entry.user_ids.contains(&user_id))
    }

    async fn insert(&self, chat_id: ChatId, user_ids: HashSet<UserId>) {
        self.entries.lock().await.insert(
            chat_id,
            CachedAdministrators {
                expires_at: Instant::now() + self.ttl,
                user_ids,
            },
        );
    }

    #[cfg(test)]
    fn with_ttl(ttl: Duration) -> Self {
        Self {
            entries: Arc::default(),
            ttl,
        }
    }
}

pub async fn observe_message_users(store: &BingoStore, message: &Message) -> Result<()> {
    let chat_id = message.chat.id.0;
    for user in message.mentioned_users() {
        if user.is_bot {
            continue;
        }
        let known = known_user(user)?;
        store.observe_user(chat_id, &known).await?;
    }
    Ok(())
}

pub async fn answer_bingo(
    bot: &Bot,
    message: &Message,
    store: &BingoStore,
    admin_cache: &AdminCache,
    input: &str,
) -> Result<()> {
    let result = execute_bingo(bot, message, store, admin_cache, input).await;
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.is_user_facing() => {
            bot.send_message(message.chat.id, format!("Bingo: {error}"))
                .await?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

pub async fn answer_callback(bot: &Bot, query: &CallbackQuery, store: &BingoStore) -> Result<()> {
    let Some(data) = query.data.as_deref() else {
        return Ok(());
    };
    let Some((card_id, position)) = parse_callback(data) else {
        return Ok(());
    };
    let user_id = telegram_user_id(&query.from)?;
    let result = store.toggle_cell(card_id, user_id, position).await;
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

async fn execute_bingo(
    bot: &Bot,
    message: &Message,
    store: &BingoStore,
    admin_cache: &AdminCache,
    input: &str,
) -> Result<()> {
    observe_message_users(store, message).await?;
    let command = BingoCommand::parse(input)?;
    if command.requires_admin() && !is_chat_admin(bot, message, admin_cache).await? {
        return Err(BingoError::PermissionDenied);
    }
    let chat_id = message.chat.id.0;

    match command {
        BingoCommand::Help => send_text(bot, message.chat.id, HELP).await,
        BingoCommand::Games => send_games(bot, message.chat.id, store).await,
        BingoCommand::Entries { slug } => send_entries(bot, message.chat.id, store, slug).await,
        BingoCommand::Get { slug, target } => {
            let owner = resolve_target(store, message, target.as_deref(), true).await?;
            let card = store.card(chat_id, slug.as_deref(), owner.user_id).await?;
            send_card(bot, message.chat.id, &card).await
        }
        BingoCommand::Game(command) => execute_game_admin(bot, message, store, command).await,
        BingoCommand::Entry(command) => {
            execute_entry_admin(bot, message.chat.id, store, command).await
        }
        BingoCommand::Card(command) => execute_card_admin(bot, message, store, command).await,
    }
}

async fn execute_game_admin(
    bot: &Bot,
    message: &Message,
    store: &BingoStore,
    command: GameAdmin,
) -> Result<()> {
    let chat_id = message.chat.id.0;
    match command {
        GameAdmin::Create { slug, name } => {
            let actor = message
                .from
                .as_ref()
                .ok_or(BingoError::PermissionDenied)
                .and_then(known_user)?;
            let game = store
                .create_game(chat_id, &slug, &name, actor.user_id)
                .await?;
            send_text(
                bot,
                message.chat.id,
                &format!("Created draft bingo game `{}` — {}.", game.slug, game.name),
            )
            .await
        }
        GameAdmin::SetState { slug, state } => {
            let game = store.set_game_state(chat_id, &slug, state).await?;
            send_text(
                bot,
                message.chat.id,
                &format!("Game `{}` is now {}.", game.slug, game.state),
            )
            .await
        }
        GameAdmin::SetDefault { slug } => {
            let game = store.set_default_game(chat_id, &slug).await?;
            send_text(
                bot,
                message.chat.id,
                &format!("Game `{}` is now the chat default.", game.slug),
            )
            .await
        }
        GameAdmin::SetCenter { slug, text } => {
            let game = store.set_center_text(chat_id, &slug, &text).await?;
            send_text(
                bot,
                message.chat.id,
                &format!(
                    "The `{}` center cell is now “{}”.",
                    game.slug, game.center_text
                ),
            )
            .await
        }
    }
}

async fn execute_entry_admin(
    bot: &Bot,
    chat_id: ChatId,
    store: &BingoStore,
    command: EntryAdmin,
) -> Result<()> {
    match command {
        EntryAdmin::Add { slug, text } => {
            let entry = store.add_entry(chat_id.0, slug.as_deref(), &text).await?;
            send_text(
                bot,
                chat_id,
                &format!("Added entry #{}: {}", entry.id, entry.text),
            )
            .await
        }
        EntryAdmin::Edit { entry_id, text } => {
            let entry = store.edit_entry(chat_id.0, entry_id, &text).await?;
            send_text(
                bot,
                chat_id,
                &format!("Updated entry #{}: {}", entry.id, entry.text),
            )
            .await
        }
        EntryAdmin::Delete { entry_id } => {
            store.delete_entry(chat_id.0, entry_id).await?;
            send_text(
                bot,
                chat_id,
                &format!("Deleted entry #{entry_id} from future cards."),
            )
            .await
        }
    }
}

async fn execute_card_admin(
    bot: &Bot,
    message: &Message,
    store: &BingoStore,
    command: CardAdmin,
) -> Result<()> {
    let chat_id = message.chat.id.0;
    match command {
        CardAdmin::Generate {
            slug,
            target,
            replace,
        } => {
            let owner = resolve_target(store, message, target.as_deref(), false).await?;
            let card = store
                .generate_card(chat_id, slug.as_deref(), &owner, replace)
                .await?;
            send_card(bot, message.chat.id, &card).await
        }
        CardAdmin::Import {
            slug,
            target,
            cells,
            replace,
        } => {
            let owner = resolve_target(store, message, target.as_deref(), false).await?;
            let card = store
                .import_card(chat_id, &slug, &owner, &cells, replace)
                .await?;
            send_card(bot, message.chat.id, &card).await
        }
        CardAdmin::Set {
            slug,
            target,
            position,
            text,
        } => {
            let owner = resolve_target(store, message, target.as_deref(), false).await?;
            let card = store
                .set_card_cell(chat_id, &slug, owner.user_id, position, &text)
                .await?;
            send_card(bot, message.chat.id, &card).await
        }
        CardAdmin::Reset { slug, target } => {
            let owner = resolve_target(store, message, target.as_deref(), false).await?;
            let card = store
                .reset_card(chat_id, slug.as_deref(), owner.user_id)
                .await?;
            send_card(bot, message.chat.id, &card).await
        }
    }
}

async fn send_games(bot: &Bot, chat_id: ChatId, store: &BingoStore) -> Result<()> {
    let games = store.list_games(chat_id.0).await?;
    let text = if games.is_empty() {
        "No bingo games have been created in this chat.".to_owned()
    } else {
        let mut lines = vec!["Bingo games:".to_owned()];
        lines.extend(games.into_iter().map(|game| {
            let default = if game.is_default { " · default" } else { "" };
            format!("{} — {} [{}{}]", game.slug, game.name, game.state, default)
        }));
        lines.join("\n")
    };
    send_text(bot, chat_id, &text).await
}

async fn send_entries(
    bot: &Bot,
    chat_id: ChatId,
    store: &BingoStore,
    slug: Option<String>,
) -> Result<()> {
    let (game, entries) = store.list_entries(chat_id.0, slug.as_deref()).await?;
    let mut lines = vec![format!("Entries for {} ({}):", game.name, entries.len())];
    lines.extend(
        entries
            .into_iter()
            .map(|entry| format!("#{} — {}", entry.id, entry.text)),
    );
    send_lines(bot, chat_id, lines).await
}

async fn is_chat_admin(bot: &Bot, message: &Message, cache: &AdminCache) -> Result<bool> {
    let Some(user) = message.from.as_ref() else {
        return Ok(false);
    };
    if message.chat.is_private() {
        return Ok(true);
    }
    if let Some(is_admin) = cache.status(message.chat.id, user.id).await {
        return Ok(is_admin);
    }
    let administrators = bot.get_chat_administrators(message.chat.id).await?;
    let user_ids = administrators
        .into_iter()
        .map(|member| member.user.id)
        .collect::<HashSet<_>>();
    let is_admin = user_ids.contains(&user.id);
    cache.insert(message.chat.id, user_ids).await;
    Ok(is_admin)
}

async fn resolve_target(
    store: &BingoStore,
    message: &Message,
    target: Option<&str>,
    default_to_actor: bool,
) -> Result<KnownUser> {
    let chat_id = message.chat.id.0;
    if let Some(target) = target {
        if let Ok(user_id) = target.parse::<i64>() {
            return store.user_by_id(chat_id, user_id).await;
        }
        if !target.starts_with('@') {
            return Err(BingoError::InvalidCommand(
                "usernames must start with `@`".to_owned(),
            ));
        }
        return store.user_by_username(chat_id, target).await;
    }
    if let Some(user) = message
        .reply_to_message()
        .and_then(|reply| reply.from.as_ref())
    {
        let user = known_user(user)?;
        store.observe_user(chat_id, &user).await?;
        return Ok(user);
    }
    if default_to_actor {
        let user = message.from.as_ref().ok_or(BingoError::PermissionDenied)?;
        return known_user(user);
    }
    Err(BingoError::InvalidCommand(
        "mention a user or reply to one of their messages".to_owned(),
    ))
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

async fn send_card(bot: &Bot, chat_id: ChatId, card: &Card) -> Result<()> {
    bot.send_message(chat_id, render_card(card))
        .reply_markup(card_keyboard(card))
        .await?;
    Ok(())
}

fn render_card(card: &Card) -> String {
    let mut lines = vec![
        format!("🏁 {}", card.game.name),
        format!("Card for {} · game: {}", card.owner, card.game.slug),
        String::new(),
    ];
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
        lines.push("🏆 BINGO — one or more lines completed!".to_owned());
    }
    lines.join("\n")
}

fn card_keyboard(card: &Card) -> InlineKeyboardMarkup {
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
                        format!("b:{}:{}", card.id, cell.position.index()),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    InlineKeyboardMarkup::new(rows)
}

fn parse_callback(data: &str) -> Option<(i64, Position)> {
    let (card_id, position) = data.strip_prefix("b:")?.split_once(':')?;
    let card_id = card_id.parse().ok()?;
    let position = position.parse::<usize>().ok()?.try_into().ok()?;
    Some((card_id, position))
}

fn known_user(user: &User) -> Result<KnownUser> {
    let display_name = user.last_name.as_ref().map_or_else(
        || user.first_name.clone(),
        |last_name| format!("{} {last_name}", user.first_name),
    );
    Ok(KnownUser {
        user_id: telegram_user_id(user)?,
        username: user.username.clone(),
        display_name,
    })
}

fn telegram_user_id(user: &User) -> Result<i64> {
    i64::try_from(user.id.0).map_err(|_| BingoError::UserIdOutOfRange(user.id.0))
}

async fn send_text(bot: &Bot, chat_id: ChatId, text: &str) -> Result<()> {
    bot.send_message(chat_id, text).await?;
    Ok(())
}

async fn send_lines(bot: &Bot, chat_id: ChatId, lines: Vec<String>) -> Result<()> {
    let mut chunk = String::new();
    for line in lines {
        if !chunk.is_empty() && chunk.len() + line.len() + 1 > TELEGRAM_MESSAGE_BUDGET {
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

    #[tokio::test]
    async fn administrator_cache_respects_expiration() {
        let chat_id = ChatId(1);
        let user_id = UserId(2);
        let cache = AdminCache::with_ttl(Duration::from_mins(1));
        cache.insert(chat_id, HashSet::from([user_id])).await;
        assert_some_eq!(cache.status(chat_id, user_id).await, true);
        assert_some_eq!(cache.status(chat_id, UserId(3)).await, false);

        let expired = AdminCache::with_ttl(Duration::ZERO);
        expired.insert(chat_id, HashSet::from([user_id])).await;
        assert_none!(expired.status(chat_id, user_id).await);
    }
}
