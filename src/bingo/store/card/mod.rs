mod queries;

use crate::bingo::{
    error::{BingoError, Result},
    model::{
        CELL_COUNT, Card, CardId, EntryId, GameState, ImportedCell, KnownUser, Position,
        REQUIRED_ENTRIES, ToggleResult, has_bingo,
    },
    store::{
        BingoStore,
        card::queries::{
            NewCell, announce_bingo, card_id_in, delete_or_reject_existing, fetch_active_entry,
            fetch_card, fetch_cells, fetch_toggle_context, find_card_id, insert_card, insert_cell,
            reset_card_state, toggle_cell_mark, update_cell,
        },
        id::db_user_id,
        validation::{ensure_active, ensure_editable, require_changed},
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
        let (entries, _) = entries.partial_shuffle(&mut rand::rng(), REQUIRED_ENTRIES);
        let mut transaction = self.pool.begin().await?;
        delete_or_reject_existing(&mut transaction, game.id, owner.user_id, replace).await?;
        let card_id = insert_card(&mut transaction, game.id, owner).await?;
        insert_cell(
            &mut transaction,
            card_id,
            NewCell::free(Position::FREE, &game.center_text),
        )
        .await?;
        for (position, entry) in Position::iter()
            .filter(|position| *position != Position::FREE)
            .zip(entries)
        {
            insert_cell(
                &mut transaction,
                card_id,
                NewCell {
                    position,
                    entry_id: Some(entry.id),
                    text: &entry.text,
                    marked: false,
                    is_free: false,
                },
            )
            .await?;
        }
        transaction.commit().await?;
        fetch_card(&self.pool, card_id).await
    }

    /// Import a card while its game is draft or active.
    ///
    /// Draft imports are supported so administrators can stage paper cards before activating a
    /// game.
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
            let position = Position::try_from(position)?;
            if position == Position::FREE {
                insert_cell(
                    &mut transaction,
                    card_id,
                    NewCell::free(position, &game.center_text),
                )
                .await?;
                continue;
            }
            let entry_id = cell.entry_id.ok_or_else(|| {
                BingoError::InvalidCommand(
                    "non-free imported cells must contain entry IDs".to_owned(),
                )
            })?;
            let entry = fetch_active_entry(&mut transaction, game.id, entry_id, slug).await?;
            insert_cell(
                &mut transaction,
                card_id,
                NewCell {
                    position,
                    entry_id: Some(entry.id),
                    text: &entry.text,
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
        let card_id = find_card_id(&self.pool, game.id, user_id)
            .await?
            .ok_or_else(|| {
                BingoError::NotFound(format!(
                    "no card is assigned to that user for `{}`",
                    game.slug
                ))
            })?;
        fetch_card(&self.pool, card_id).await
    }

    pub async fn set_card_cell(
        &self,
        chat_id: ChatId,
        slug: &str,
        user_id: UserId,
        position: Position,
        entry_id: EntryId,
    ) -> Result<Card> {
        if position == Position::FREE {
            return Err(BingoError::FreeCell);
        }
        let game = self.game(chat_id, Some(slug)).await?;
        ensure_editable(&game)?;
        let mut transaction = self.pool.begin().await?;
        let card_id = card_id_in(&mut transaction, game.id, user_id).await?;
        let entry = fetch_active_entry(&mut transaction, game.id, entry_id, slug).await?;
        update_cell(&mut transaction, card_id, position, entry).await?;
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
        ensure_editable(&game)?;
        let mut transaction = self.pool.begin().await?;
        let card_id = card_id_in(&mut transaction, game.id, user_id).await?;
        reset_card_state(&mut transaction, card_id).await?;
        transaction.commit().await?;
        fetch_card(&self.pool, card_id).await
    }

    pub async fn toggle_cell(
        &self,
        card_id: CardId,
        user_id: UserId,
        position: Position,
    ) -> Result<ToggleResult> {
        if position == Position::FREE {
            return Err(BingoError::FreeCell);
        }
        let mut transaction = self.pool.begin().await?;
        let context = fetch_toggle_context(&mut transaction, card_id).await?;
        if context.user_id != db_user_id(user_id)? {
            return Err(BingoError::NotCardOwner);
        }
        if context.state.parse::<GameState>()? != GameState::Active {
            return Err(BingoError::GameNotActive);
        }
        let rows_affected = toggle_cell_mark(&mut transaction, card_id, position).await?;
        require_changed(rows_affected, || "that card cell was not found".to_owned())?;
        let cells = fetch_cells(&mut *transaction, card_id).await?;
        let newly_completed = !context.bingo_announced && has_bingo(&cells);
        if newly_completed {
            announce_bingo(&mut transaction, card_id).await?;
        }
        transaction.commit().await?;
        let card = fetch_card(&self.pool, card_id).await?;
        Ok(ToggleResult {
            card,
            newly_completed,
        })
    }
}
