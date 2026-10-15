use crate::bingo::{
    error::{BingoError, Result},
    model::{
        CELL_COUNT, Card, CardCell, CardId, EntryId, EntryNumber, Game, GameId, KnownUser, Position,
    },
    store::id::{db_user_id, user_id_from_db},
};
use sqlx::{Sqlite, SqliteConnection, SqliteExecutor, Transaction};
use std::cmp::Ordering;
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
    known_username: Option<String>,
    known_display_name: Option<String>,
}

impl CardRow {
    fn into_card(self, cells: [CardCell; CELL_COUNT]) -> Result<Card> {
        let (username, display_name) = self.known_display_name.map_or_else(
            || (None, self.owner_name),
            |display_name| (self.known_username, display_name),
        );
        let owner = KnownUser {
            user_id: user_id_from_db(self.user_id)?,
            username,
            display_name,
        };

        Ok(Card {
            id: self.card_id.into(),
            game: Game {
                id: self.game_id.into(),
                chat_id: ChatId(self.chat_id),
                slug: self.slug,
                name: self.game_name,
                description: self.description,
                center_text: self.center_text,
                state: self.state.parse()?,
                is_default: self.is_default,
            },
            owner,
            bingo_announced: self.bingo_announced,
            cells,
        })
    }
}

#[derive(Debug)]
struct CellRow {
    position: i64,
    text: String,
    marked: bool,
    is_free: bool,
}

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

/// The error for a card that does not exist, also used for cards outside the caller's chat so
/// their ids are not revealed.
pub fn card_not_found() -> BingoError {
    BingoError::NotFound("That bingo card no longer exists".to_owned())
}

pub async fn fetch_card(connection: &mut SqliteConnection, card_id: CardId) -> Result<Card> {
    let row = sqlx::query_as!(
        CardRow,
        r#"
        SELECT
            c.id AS card_id,
            c.user_id,
            c.owner_name,
            c.bingo_announced AS `bingo_announced: bool`,
            g.id AS game_id,
            g.chat_id,
            g.slug,
            g.name AS game_name,
            g.description,
            g.center_text,
            g.state,
            g.is_default AS `is_default: bool`,
            u.username AS `known_username?`,
            u.display_name AS `known_display_name?`
        FROM
            bingo_cards c
            JOIN bingo_games g ON g.id = c.game_id
            LEFT JOIN bingo_users u ON u.chat_id = g.chat_id
            AND u.user_id = c.user_id
        WHERE
            c.id = ?
        "#,
        card_id.get(),
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or_else(card_not_found)?;
    let cells = fetch_cells(connection, card_id).await?;
    row.into_card(cells)
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

async fn fetch_cells(
    connection: &mut SqliteConnection,
    card_id: CardId,
) -> Result<[CardCell; CELL_COUNT]> {
    let rows = sqlx::query_as!(
        CellRow,
        r#"
        SELECT
            position,
            text,
            marked AS `marked: bool`,
            is_free AS `is_free: bool`
        FROM
            bingo_card_cells
        WHERE
            card_id = ?
        ORDER BY
            position
        "#,
        card_id.get(),
    )
    .fetch_all(connection)
    .await?;
    convert_cells(rows)
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

/// Converts rows sorted by position into a card's cells, rejecting missing, extra or duplicate
/// cells.
fn convert_cells(rows: Vec<CellRow>) -> Result<[CardCell; CELL_COUNT]> {
    let cells = rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let position = Position::try_from(row.position)
                .map_err(|_| BingoError::InvalidStoredCellPosition(row.position))?;
            match position.index().cmp(&index) {
                Ordering::Less => {
                    return Err(BingoError::InvalidCardLayout(format!(
                        "Cell {position} appears more than once"
                    )));
                }
                Ordering::Greater => {
                    return Err(BingoError::InvalidCardLayout(format!(
                        "Missing cell before {position}"
                    )));
                }
                Ordering::Equal => {}
            }
            Ok(CardCell {
                position,
                text: row.text,
                marked: row.marked,
                is_free: row.is_free,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    <[CardCell; CELL_COUNT]>::try_from(cells).map_err(|cells| {
        BingoError::InvalidCardLayout(format!(
            "Expected {CELL_COUNT} cells, found {}",
            cells.len()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_ok, assert_some};
    use std::assert_matches;

    fn rows() -> Vec<CellRow> {
        (0..CELL_COUNT)
            .map(|index| CellRow {
                position: assert_ok!(i64::try_from(index)),
                text: format!("Cell {index}"),
                marked: false,
                is_free: false,
            })
            .collect()
    }

    #[test]
    fn indexes_cells_by_stored_position() {
        let cells = assert_ok!(convert_cells(rows()));
        assert_eq!(cells[0].position.to_string(), "A1");
        assert_eq!(cells[CELL_COUNT - 1].position.to_string(), "E5");
    }

    #[test]
    fn rejects_missing_cells() {
        let mut missing = rows();
        missing.pop();
        assert_matches!(
            convert_cells(missing),
            Err(BingoError::InvalidCardLayout(_))
        );

        let mut gap = rows();
        gap.remove(1);
        assert_matches!(convert_cells(gap), Err(BingoError::InvalidCardLayout(_)));
    }

    #[test]
    fn rejects_duplicate_and_extra_cells() {
        let mut duplicate = rows();
        assert_some!(duplicate.get_mut(1)).position = 0;
        assert_matches!(
            convert_cells(duplicate),
            Err(BingoError::InvalidCardLayout(_))
        );

        let mut extra = rows();
        extra.push(CellRow {
            position: 24,
            text: String::new(),
            marked: false,
            is_free: false,
        });
        assert_matches!(convert_cells(extra), Err(BingoError::InvalidCardLayout(_)));
    }
}
