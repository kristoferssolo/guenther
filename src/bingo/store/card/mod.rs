mod queries;

use crate::bingo::{
    error::{BingoError, Result},
    model::{
        CELL_COUNT, Card, GameState, ImportedCell, KnownUser, MAX_ENTRY_CHARS, Position,
        REQUIRED_ENTRIES, ToggleResult, has_bingo,
    },
    store::{
        BingoStore,
        card::queries::{
            NewCell, card_id_in, delete_or_reject_existing, fetch_card, fetch_cells, insert_card,
            insert_cell,
        },
        entry::upsert_entry,
        id::db_user_id,
        validation::{ensure_active, ensure_editable, require_changed, validate_nonempty},
    },
};
use rand::seq::SliceRandom;
use teloxide::types::{ChatId, UserId};

impl BingoStore {
    pub async fn generate_card(
        &self,
        chat_id: ChatId,
        slug: Option<&str>,
        owner: &KnownUser,
        replace: bool,
    ) -> Result<Card> {
        let (game, mut entries) = self.list_entries(chat_id, slug).await?;
        ensure_active(&game)?;
        if entries.len() < REQUIRED_ENTRIES {
            return Err(BingoError::Conflict(format!(
                "game `{}` needs {REQUIRED_ENTRIES} active entries but only has {}",
                game.slug,
                entries.len()
            )));
        }
        entries.shuffle(&mut rand::rng());
        entries.truncate(REQUIRED_ENTRIES);
        let mut transaction = self.pool.begin().await?;
        delete_or_reject_existing(&mut transaction, game.id, owner.user_id, replace).await?;
        let card_id = insert_card(&mut transaction, game.id, owner).await?;
        let mut entries = entries.iter();
        for position in Position::iter() {
            let cell = if position == Position::FREE {
                NewCell {
                    position,
                    entry_id: None,
                    text: &game.center_text,
                    marked: true,
                    is_free: true,
                }
            } else {
                let entry = entries
                    .next()
                    .expect("entry count was checked before generating the card");
                NewCell {
                    position,
                    entry_id: Some(entry.id),
                    text: &entry.text,
                    marked: false,
                    is_free: false,
                }
            };
            insert_cell(&mut transaction, card_id, cell).await?;
        }
        transaction.commit().await?;
        fetch_card(&self.pool, card_id).await
    }

    pub async fn import_card(
        &self,
        chat_id: ChatId,
        slug: &str,
        owner: &KnownUser,
        imported: &[ImportedCell],
        replace: bool,
    ) -> Result<Card> {
        if imported.len() != CELL_COUNT {
            return Err(BingoError::InvalidCommand(format!(
                "an imported card must contain {CELL_COUNT} cells"
            )));
        }
        let game = self.game(chat_id, Some(slug)).await?;
        ensure_editable(&game)?;
        let mut transaction = self.pool.begin().await?;
        delete_or_reject_existing(&mut transaction, game.id, owner.user_id, replace).await?;
        let card_id = insert_card(&mut transaction, game.id, owner).await?;
        for (position, cell) in imported.iter().enumerate() {
            let position = Position::try_from(position)
                .expect("import length was validated before inserting cells");
            if position == Position::FREE {
                insert_cell(
                    &mut transaction,
                    card_id,
                    NewCell {
                        position,
                        entry_id: None,
                        text: &game.center_text,
                        marked: true,
                        is_free: true,
                    },
                )
                .await?;
                continue;
            }
            let entry_id = upsert_entry(&mut *transaction, game.id, &cell.text).await?;
            insert_cell(
                &mut transaction,
                card_id,
                NewCell {
                    position,
                    entry_id: Some(entry_id),
                    text: &cell.text,
                    marked: cell.marked,
                    is_free: false,
                },
            )
            .await?;
        }
        transaction.commit().await?;
        fetch_card(&self.pool, card_id).await
    }

    pub async fn card(&self, chat_id: ChatId, slug: Option<&str>, user_id: UserId) -> Result<Card> {
        let game = self.game(chat_id, slug).await?;
        let user_id = db_user_id(user_id)?;
        let card_id = sqlx::query_scalar!(
            r#"SELECT id FROM bingo_cards WHERE game_id = ? AND user_id = ?"#,
            game.id,
            user_id,
        )
        .fetch_optional(&self.pool)
        .await?;
        fetch_card(
            &self.pool,
            card_id.ok_or_else(|| {
                BingoError::NotFound(format!(
                    "no card is assigned to that user for `{}`",
                    game.slug
                ))
            })?,
        )
        .await
    }

    pub async fn set_card_cell(
        &self,
        chat_id: ChatId,
        slug: &str,
        user_id: UserId,
        position: Position,
        text: &str,
    ) -> Result<Card> {
        if position == Position::FREE {
            return Err(BingoError::FreeCell);
        }
        validate_nonempty(text, "cell", MAX_ENTRY_CHARS)?;
        let game = self.game(chat_id, Some(slug)).await?;
        ensure_editable(&game)?;
        let mut transaction = self.pool.begin().await?;
        let card_id = card_id_in(&mut transaction, game.id, user_id).await?;
        let entry_id = upsert_entry(&mut *transaction, game.id, text).await?;
        let position = i64::from(position);
        sqlx::query!(
            r#"UPDATE bingo_card_cells SET entry_id = ?, text = ?
WHERE card_id = ? AND position = ?"#,
            entry_id,
            text.trim(),
            card_id,
            position,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        fetch_card(&self.pool, card_id).await
    }

    pub async fn reset_card(
        &self,
        chat_id: ChatId,
        slug: Option<&str>,
        user_id: UserId,
    ) -> Result<Card> {
        let game = self.game(chat_id, slug).await?;
        let mut transaction = self.pool.begin().await?;
        let card_id = card_id_in(&mut transaction, game.id, user_id).await?;
        sqlx::query!(
            r#"UPDATE bingo_card_cells SET marked = is_free WHERE card_id = ?"#,
            card_id,
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query!(
            r#"UPDATE bingo_cards SET bingo_announced = 0 WHERE id = ?"#,
            card_id,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        fetch_card(&self.pool, card_id).await
    }

    pub async fn toggle_cell(
        &self,
        card_id: i64,
        user_id: UserId,
        position: Position,
    ) -> Result<ToggleResult> {
        if position == Position::FREE {
            return Err(BingoError::FreeCell);
        }
        let position = i64::from(position);
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query!(
            r#"SELECT
    c.user_id, c.bingo_announced AS `bingo_announced: bool`,
    g.state FROM bingo_cards c
    JOIN bingo_games g ON g.id = c.game_id
WHERE c.id = ?"#,
            card_id,
        )
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| BingoError::NotFound("that bingo card no longer exists".to_owned()))?;
        if row.user_id != db_user_id(user_id)? {
            return Err(BingoError::NotCardOwner);
        }
        if row.state.parse::<GameState>()? != GameState::Active {
            return Err(BingoError::GameNotActive);
        }
        let result = sqlx::query!(
            r#"UPDATE bingo_card_cells SET marked = NOT marked
WHERE card_id = ? AND position = ? AND is_free = 0"#,
            card_id,
            position,
        )
        .execute(&mut *transaction)
        .await?;
        require_changed(result.rows_affected(), || {
            "that card cell was not found".to_owned()
        })?;
        let cells = fetch_cells(&mut *transaction, card_id).await?;
        let newly_completed = !row.bingo_announced && has_bingo(&cells);
        if newly_completed {
            sqlx::query!(
                r#"UPDATE bingo_cards SET bingo_announced = 1 WHERE id = ?"#,
                card_id,
            )
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        let card = fetch_card(&self.pool, card_id).await?;
        Ok(ToggleResult {
            card,
            newly_completed,
        })
    }
}
