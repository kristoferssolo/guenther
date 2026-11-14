use crate::bingo::{
    error::{BingoError, Result},
    model::{CardId, EntryId, EntryNumber, GameId, KnownUser, Position},
    store::id::db_user_id,
};
use sqlx::{Sqlite, SqliteExecutor, Transaction};
use teloxide::types::UserId;

#[derive(Debug)]
pub struct ActiveEntry {
    pub id: EntryId,
    pub text: String,
}

pub struct NewCell<'a> {
    pub position: Position,
    pub entry_id: Option<EntryId>,
    pub text: &'a str,
    pub marked: bool,
    pub is_free: bool,
}

impl<'a> NewCell<'a> {
    pub const fn free(position: Position, text: &'a str) -> Self {
        Self {
            position,
            entry_id: None,
            text,
            marked: true,
            is_free: true,
        }
    }
}

pub async fn delete_or_reject_existing(
    transaction: &mut Transaction<'_, Sqlite>,
    game_id: GameId,
    user_id: UserId,
    replace: bool,
) -> Result<()> {
    if let Some(card_id) = find_card_id(&mut **transaction, game_id, user_id).await? {
        if !replace {
            return Err(BingoError::Conflict(
                "That user already has a card; use `/bingo regenerate` or `/bingo reimport` to replace it"
                    .to_owned(),
            ));
        }
        sqlx::query!(
            r#"
            DELETE FROM
                bingo_cards
            WHERE
                id = ?
            "#,
            card_id.get()
        )
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

pub async fn find_card_id<'e>(
    executor: impl SqliteExecutor<'e>,
    game_id: GameId,
    user_id: UserId,
) -> Result<Option<CardId>> {
    let user_id = db_user_id(user_id)?;
    Ok(sqlx::query_scalar!(
        r#"
        SELECT
            id
        FROM
            bingo_cards
        WHERE
            game_id = ?
            AND user_id = ?
        "#,
        game_id.get(),
        user_id,
    )
    .fetch_optional(executor)
    .await?
    .map(CardId::from))
}

pub async fn fetch_active_entry(
    transaction: &mut Transaction<'_, Sqlite>,
    game_id: GameId,
    entry_number: EntryNumber,
    slug: &str,
) -> Result<ActiveEntry> {
    sqlx::query!(
        r#"
        SELECT
            id,
            text
        FROM
            bingo_entries
        WHERE
            number = ?
            AND game_id = ?
            AND active = 1
        "#,
        entry_number.get(),
        game_id.get(),
    )
    .fetch_optional(&mut **transaction)
    .await?
    .map(|row| ActiveEntry {
        id: row.id.into(),
        text: row.text,
    })
    .ok_or_else(|| {
        BingoError::NotFound(format!(
            "Active entry `{entry_number}` was not found in game `{slug}`"
        ))
    })
}

pub async fn insert_card(
    transaction: &mut Transaction<'_, Sqlite>,
    game_id: GameId,
    owner: &KnownUser,
) -> Result<CardId> {
    let owner_name = owner.to_string();
    let user_id = db_user_id(owner.user_id)?;
    let result = sqlx::query!(
        r#"
        INSERT INTO
            bingo_cards (game_id, user_id, owner_name)
        VALUES
            (?, ?, ?)
        "#,
        game_id.get(),
        user_id,
        owner_name,
    )
    .execute(&mut **transaction)
    .await?;
    Ok(result.last_insert_rowid().into())
}

pub async fn insert_cell(
    transaction: &mut Transaction<'_, Sqlite>,
    card_id: CardId,
    cell: NewCell<'_>,
) -> Result<()> {
    let NewCell {
        position,
        entry_id,
        text,
        marked,
        is_free,
    } = cell;
    let position = i64::from(position);
    let entry_id = entry_id.map(EntryId::get);
    let text = text.trim();
    sqlx::query!(
        r#"
        INSERT INTO
            bingo_card_cells (
                card_id,
                position,
                entry_id,
                text,
                marked,
                is_free
            )
        VALUES
            (?, ?, ?, ?, ?, ?)
        "#,
        card_id.get(),
        position,
        entry_id,
        text,
        marked,
        is_free,
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub async fn update_cell(
    transaction: &mut Transaction<'_, Sqlite>,
    card_id: CardId,
    position: Position,
    entry: ActiveEntry,
) -> Result<()> {
    let position = i64::from(position);
    sqlx::query!(
        r#"
        UPDATE
            bingo_card_cells
        SET
            entry_id = ?,
            text = ?
        WHERE
            card_id = ?
            AND position = ?
        "#,
        entry.id.get(),
        entry.text,
        card_id.get(),
        position,
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub async fn reset_card_state(
    transaction: &mut Transaction<'_, Sqlite>,
    card_id: CardId,
) -> Result<()> {
    sqlx::query!(
        r#"
        UPDATE
            bingo_card_cells
        SET
            marked = is_free
        WHERE
            card_id = ?
        "#,
        card_id.get(),
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query!(
        r#"
        UPDATE
            bingo_cards
        SET
            bingo_announced = 0
        WHERE
            id = ?
        "#,
        card_id.get(),
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub async fn toggle_cell_mark(
    transaction: &mut Transaction<'_, Sqlite>,
    card_id: CardId,
    position: Position,
) -> Result<()> {
    let position = i64::from(position);
    sqlx::query!(
        r#"
        UPDATE
            bingo_card_cells
        SET
            marked = NOT marked
        WHERE
            card_id = ?
            AND position = ?
            AND is_free = 0
        "#,
        card_id.get(),
        position,
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub async fn announce_bingo(
    transaction: &mut Transaction<'_, Sqlite>,
    card_id: CardId,
) -> Result<()> {
    sqlx::query!(
        r#"
        UPDATE
            bingo_cards
        SET
            bingo_announced = 1
        WHERE
            id = ?
        "#,
        card_id.get(),
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub async fn card_id_in(
    transaction: &mut Transaction<'_, Sqlite>,
    game_id: GameId,
    user_id: UserId,
) -> Result<CardId> {
    find_card_id(&mut **transaction, game_id, user_id)
        .await?
        .ok_or_else(|| {
            BingoError::NotFound("That user does not have a card for this game".to_owned())
        })
}
