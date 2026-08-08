use crate::bingo::{
    error::{BingoError, Result},
    model::{Entry, EntryId, EntryNumber, Game, GameId, MAX_ENTRY_CHARS, normalize_entry},
    store::{
        BingoStore,
        validation::{ensure_editable, map_unique, require_changed, validate_nonempty},
    },
};
use sqlx::SqliteExecutor;
use std::collections::HashSet;
use teloxide::types::ChatId;

#[derive(Debug)]
struct EntryRow {
    id: i64,
    number: i64,
    game_id: i64,
    text: String,
}

#[derive(Debug)]
struct UpsertedEntry {
    id: EntryId,
    number: EntryNumber,
}

impl BingoStore {
    pub async fn add_entry(
        &self,
        chat_id: ChatId,
        slug: Option<&str>,
        text: &str,
    ) -> Result<Entry> {
        let game = self.game(chat_id, slug).await?;
        ensure_editable(&game)?;
        let upserted = upsert_entry(&self.pool, game.id, text).await?;
        Ok(Entry {
            id: upserted.id,
            number: upserted.number,
            game_id: game.id,
            text: text.trim().to_owned(),
        })
    }

    pub async fn list_entries(
        &self,
        chat_id: ChatId,
        slug: Option<&str>,
    ) -> Result<(Game, Vec<Entry>)> {
        let game = self.game(chat_id, slug).await?;
        let rows = sqlx::query_as!(
            EntryRow,
            r#"SELECT id, number, game_id, text FROM bingo_entries
WHERE game_id = ? AND active = 1 ORDER BY number"#,
            game.id.get(),
        )
        .fetch_all(&self.pool)
        .await?;
        let entries = rows
            .into_iter()
            .map(Entry::try_from)
            .collect::<Result<Vec<_>>>()?;
        Ok((game, entries))
    }

    pub async fn import_entries(
        &self,
        chat_id: ChatId,
        slug: &str,
        entries: &[String],
    ) -> Result<usize> {
        if entries.is_empty() {
            return Err(BingoError::InvalidCommand(
                "the entry file does not contain any entries".to_owned(),
            ));
        }
        let game = self.game(chat_id, Some(slug)).await?;
        ensure_editable(&game)?;
        let mut transaction = self.pool.begin().await?;
        let mut imported_ids = HashSet::with_capacity(entries.len());
        for entry in entries {
            let upserted = upsert_entry(&mut *transaction, game.id, entry).await?;
            imported_ids.insert(upserted.id);
        }
        transaction.commit().await?;
        Ok(imported_ids.len())
    }

    pub async fn edit_entry(
        &self,
        chat_id: ChatId,
        slug: Option<&str>,
        entry_number: EntryNumber,
        text: &str,
    ) -> Result<Entry> {
        validate_nonempty(text, "entry", MAX_ENTRY_CHARS)?;
        let game = self.game(chat_id, slug).await?;
        ensure_editable(&game)?;
        let normalized = normalize_entry(text);
        let row = sqlx::query_as!(
            EntryRow,
            r#"UPDATE bingo_entries SET text = ?, normalized_text = ?
WHERE game_id = ? AND number = ? AND active = 1
RETURNING id, number, game_id, text"#,
            text.trim(),
            normalized,
            game.id.get(),
            entry_number.get(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_unique(error, || "that entry already exists".to_owned()))?
        .ok_or_else(|| {
            BingoError::NotFound(format!(
                "entry `{entry_number}` was not found in game `{}`",
                game.slug
            ))
        })?;
        row.try_into()
    }

    pub async fn delete_entry(
        &self,
        chat_id: ChatId,
        slug: Option<&str>,
        entry_number: EntryNumber,
    ) -> Result<()> {
        let game = self.game(chat_id, slug).await?;
        ensure_editable(&game)?;
        let result = sqlx::query!(
            r#"UPDATE bingo_entries SET active = 0
WHERE game_id = ? AND number = ? AND active = 1"#,
            game.id.get(),
            entry_number.get(),
        )
        .execute(&self.pool)
        .await?;
        require_changed(result.rows_affected(), || {
            format!(
                "entry `{entry_number}` was not found in game `{}`",
                game.slug
            )
        })
    }
}

impl TryFrom<EntryRow> for Entry {
    type Error = BingoError;

    fn try_from(row: EntryRow) -> Result<Self> {
        Ok(Self {
            id: row.id.into(),
            number: row.number.try_into()?,
            game_id: row.game_id.into(),
            text: row.text,
        })
    }
}

async fn upsert_entry<'e>(
    executor: impl SqliteExecutor<'e>,
    game_id: GameId,
    text: &str,
) -> Result<UpsertedEntry> {
    validate_nonempty(text, "entry", MAX_ENTRY_CHARS)?;
    let normalized = normalize_entry(text);
    let text = text.trim();
    let row = sqlx::query!(
        r#"INSERT INTO bingo_entries (game_id, number, text, normalized_text)
VALUES (
    ?,
    (SELECT COALESCE(MAX(number), 0) + 1 FROM bingo_entries WHERE game_id = ?),
    ?,
    ?
)
ON CONFLICT(game_id, normalized_text)
DO UPDATE SET text = excluded.text, active = 1
RETURNING id, number"#,
        game_id.get(),
        game_id.get(),
        text,
        normalized,
    )
    .fetch_one(executor)
    .await?;
    Ok(UpsertedEntry {
        id: row.id.into(),
        number: row.number.try_into()?,
    })
}
