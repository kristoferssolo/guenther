use crate::bingo::{
    command::{BingoCommand, CardAdmin, EntryAdmin, GameAdmin},
    error::{BingoError, Result},
    model::KnownUser,
    store::BingoStore,
    telegram::{
        admin::{AdminCache, is_chat_admin},
        known_user, observe_message_users,
        render::{HELP, send_card, send_entries, send_games, send_text},
    },
};
use teloxide::{
    prelude::{Bot, ChatId},
    types::{Message, UserId},
};

pub(in crate::bingo::telegram) async fn execute_bingo(
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
    let chat_id = message.chat.id;

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
    let chat_id = message.chat.id;
    match command {
        GameAdmin::Create { slug, name } => {
            let actor = message
                .from
                .as_ref()
                .ok_or(BingoError::PermissionDenied)
                .map(known_user)?;
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
            let entry = store.add_entry(chat_id, slug.as_deref(), &text).await?;
            send_text(
                bot,
                chat_id,
                &format!("Added entry #{}: {}", entry.id, entry.text),
            )
            .await
        }
        EntryAdmin::Edit { entry_id, text } => {
            let entry = store.edit_entry(chat_id, entry_id, &text).await?;
            send_text(
                bot,
                chat_id,
                &format!("Updated entry #{}: {}", entry.id, entry.text),
            )
            .await
        }
        EntryAdmin::Delete { entry_id } => {
            store.delete_entry(chat_id, entry_id).await?;
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
    let chat_id = message.chat.id;
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

async fn resolve_target(
    store: &BingoStore,
    message: &Message,
    target: Option<&str>,
    default_to_actor: bool,
) -> Result<KnownUser> {
    let chat_id = message.chat.id;
    if let Some(target) = target {
        if let Ok(user_id) = target.parse::<u64>() {
            return store.user_by_id(chat_id, UserId(user_id)).await;
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
        let user = known_user(user);
        store.observe_user(chat_id, &user).await?;
        return Ok(user);
    }
    if default_to_actor {
        let user = message.from.as_ref().ok_or(BingoError::PermissionDenied)?;
        return Ok(known_user(user));
    }
    Err(BingoError::InvalidCommand(
        "mention a user or reply to one of their messages".to_owned(),
    ))
}
