use crate::bingo::{
    error::{BingoError, Result},
    model::{Entry, Game, MAX_ENTRY_CHARS, normalize_entry},
    store::{
        BingoStore,
        validation::{ensure_editable, map_unique, require_changed, validate_nonempty},
    },
};
use sqlx::SqliteExecutor;
use teloxide::types::ChatId;

#[derive(Debug)]
struct EntryRow {
    id: i64,
    game_id: i64,
    text: String,
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
        let id = upsert_entry(&self.pool, game.id, text).await?;
        Ok(Entry {
            id,
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
            r#"SELECT id, game_id, text FROM bingo_entries
WHERE game_id = ? AND active = 1 ORDER BY id"#,
            game.id,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok((game, rows.into_iter().map(Entry::from).collect()))
    }

    pub async fn edit_entry(&self, chat_id: ChatId, entry_id: i64, text: &str) -> Result<Entry> {
        validate_nonempty(text, "entry", MAX_ENTRY_CHARS)?;
        let normalized = normalize_entry(text);
        let chat_id = chat_id.0;
        let row = sqlx::query_as!(
            EntryRow,
            r#"UPDATE bingo_entries SET text = ?, normalized_text = ?
WHERE id = ? AND game_id IN
(SELECT id FROM bingo_games WHERE chat_id = ? AND state != 'closed') AND active = 1
RETURNING id, game_id, text"#,
            text.trim(),
            normalized,
            entry_id,
            chat_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_unique(error, || "that entry already exists".to_owned()))?
        .ok_or_else(|| BingoError::NotFound(format!("entry `{entry_id}` was not found")))?;
        Ok(row.into())
    }

    pub async fn delete_entry(&self, chat_id: ChatId, entry_id: i64) -> Result<()> {
        let chat_id = chat_id.0;
        let result = sqlx::query!(
            r#"UPDATE bingo_entries SET active = 0
WHERE id = ? AND game_id IN
(SELECT id FROM bingo_games WHERE chat_id = ? AND state != 'closed') AND active = 1"#,
            entry_id,
            chat_id,
        )
        .execute(&self.pool)
        .await?;
        require_changed(result.rows_affected(), || {
            format!("entry `{entry_id}` was not found")
        })
    }
}

impl From<EntryRow> for Entry {
    fn from(row: EntryRow) -> Self {
        Self {
            id: row.id,
            game_id: row.game_id,
            text: row.text,
        }
    }
}

pub async fn upsert_entry<'e>(
    executor: impl SqliteExecutor<'e>,
    game_id: i64,
    text: &str,
) -> Result<i64> {
    validate_nonempty(text, "entry", MAX_ENTRY_CHARS)?;
    let normalized = normalize_entry(text);
    let text = text.trim();
    Ok(sqlx::query_scalar!(
        r#"INSERT INTO bingo_entries (game_id, text, normalized_text)
VALUES (?, ?, ?)
ON CONFLICT(game_id, normalized_text)
DO UPDATE SET text = excluded.text, active = 1
RETURNING id"#,
        game_id,
        text,
        normalized,
    )
    .fetch_one(executor)
    .await?)
}
