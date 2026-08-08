use crate::bingo::{
    error::{BingoError, Result},
    model::{Card, CardCell, Game, KnownUser, Position},
    store::id::{db_user_id, user_id_from_db},
};
use sqlx::{Sqlite, SqliteExecutor, SqlitePool, Transaction};
use teloxide::types::{ChatId, UserId};

#[derive(Debug)]
struct CardRow {
    card_id: i64,
    user_id: i64,
    owner_name: String,
    bingo_announced: bool,
    game_id: i64,
    chat_id: i64,
    slug: String,
    game_name: String,
    description: String,
    center_text: String,
    state: String,
    is_default: bool,
}

#[derive(Debug)]
struct CellRow {
    position: i64,
    text: String,
    marked: bool,
    is_free: bool,
}

pub struct NewCell<'a> {
    pub position: Position,
    pub entry_id: Option<i64>,
    pub text: &'a str,
    pub marked: bool,
    pub is_free: bool,
}

pub async fn fetch_card(pool: &SqlitePool, card_id: i64) -> Result<Card> {
    let row = sqlx::query_as!(
        CardRow,
        r#"SELECT
    c.id AS card_id, c.user_id, c.owner_name,
    c.bingo_announced AS `bingo_announced: bool`,
    g.id AS game_id, g.chat_id, g.slug, g.name AS game_name, g.description, g.center_text,
    g.state, g.is_default AS `is_default: bool`
FROM bingo_cards c JOIN bingo_games g ON g.id = c.game_id WHERE c.id = ?"#,
        card_id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| BingoError::NotFound("that bingo card no longer exists".to_owned()))?;
    let known_user = sqlx::query!(
        r#"SELECT user_id, username, display_name FROM bingo_users
WHERE chat_id = ? AND user_id = ?"#,
        row.chat_id,
        row.user_id,
    )
    .fetch_optional(pool)
    .await?;
    let user_id = user_id_from_db(row.user_id)?;
    let user = match known_user {
        Some(known) => KnownUser {
            user_id,
            username: known.username,
            display_name: known.display_name,
        },
        None => KnownUser {
            user_id,
            username: None,
            display_name: row.owner_name,
        },
    };
    let cells = fetch_cells(pool, card_id).await?;
    Ok(Card {
        id: row.card_id,
        game: Game {
            id: row.game_id,
            chat_id: ChatId(row.chat_id),
            slug: row.slug,
            name: row.game_name,
            description: row.description,
            center_text: row.center_text,
            state: row.state.parse()?,
            is_default: row.is_default,
        },
        owner: user,
        bingo_announced: row.bingo_announced,
        cells,
    })
}

pub async fn delete_or_reject_existing(
    transaction: &mut Transaction<'_, Sqlite>,
    game_id: i64,
    user_id: UserId,
    replace: bool,
) -> Result<()> {
    let user_id = db_user_id(user_id)?;
    let existing = sqlx::query_scalar!(
        r#"SELECT id FROM bingo_cards WHERE game_id = ? AND user_id = ?"#,
        game_id,
        user_id,
    )
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some(card_id) = existing {
        if !replace {
            return Err(BingoError::Conflict(
                "that user already has a card; use regenerate or reimport to replace it".to_owned(),
            ));
        }
        sqlx::query!(r#"DELETE FROM bingo_cards WHERE id = ?"#, card_id)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

pub async fn insert_card(
    transaction: &mut Transaction<'_, Sqlite>,
    game_id: i64,
    owner: &KnownUser,
) -> Result<i64> {
    let owner_name = owner.to_string();
    let user_id = db_user_id(owner.user_id)?;
    let result = sqlx::query!(
        r#"INSERT INTO bingo_cards (game_id, user_id, owner_name) VALUES (?, ?, ?)"#,
        game_id,
        user_id,
        owner_name,
    )
    .execute(&mut **transaction)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn insert_cell(
    transaction: &mut Transaction<'_, Sqlite>,
    card_id: i64,
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
    let text = text.trim();
    sqlx::query!(
        r#"INSERT INTO bingo_card_cells
(card_id, position, entry_id, text, marked, is_free) VALUES (?, ?, ?, ?, ?, ?)"#,
        card_id,
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

pub async fn fetch_cells<'e>(
    executor: impl SqliteExecutor<'e>,
    card_id: i64,
) -> Result<Vec<CardCell>> {
    let rows = sqlx::query_as!(
        CellRow,
        r#"SELECT
    position, text, marked AS `marked: bool`,
    is_free AS `is_free: bool`
FROM bingo_card_cells
WHERE card_id = ? ORDER BY position"#,
        card_id,
    )
    .fetch_all(executor)
    .await?;
    convert_cells(rows)
}

pub async fn card_id_in(
    transaction: &mut Transaction<'_, Sqlite>,
    game_id: i64,
    user_id: UserId,
) -> Result<i64> {
    let user_id = db_user_id(user_id)?;
    sqlx::query_scalar!(
        r#"SELECT id FROM bingo_cards WHERE game_id = ? AND user_id = ?"#,
        game_id,
        user_id,
    )
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| BingoError::NotFound("that user does not have a card for this game".to_owned()))
}

fn convert_cells(rows: Vec<CellRow>) -> Result<Vec<CardCell>> {
    rows.into_iter()
        .map(|row| {
            let position = Position::try_from(row.position).map_err(|_| {
                BingoError::Conflict("database contains an invalid cell position".to_owned())
            })?;
            Ok(CardCell {
                position,
                text: row.text,
                marked: row.marked,
                is_free: row.is_free,
            })
        })
        .collect()
}
