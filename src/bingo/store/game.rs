use crate::bingo::{
    error::{BingoError, Result},
    model::{Game, GameState, MAX_GAME_DESCRIPTION_CHARS},
    store::{
        BingoStore,
        id::db_user_id,
        validation::{ensure_editable, map_unique, validate_nonempty, validate_slug},
    },
};
use sqlx::{FromRow, SqliteExecutor, query, query_as, query_scalar};
use teloxide::types::{ChatId, UserId};

#[derive(Debug, FromRow)]
struct GameRow {
    id: i64,
    chat_id: i64,
    slug: String,
    name: String,
    description: String,
    center_text: String,
    state: String,
    is_default: bool,
}

impl BingoStore {
    pub async fn create_game(
        &self,
        chat_id: ChatId,
        slug: &str,
        name: &str,
        created_by: UserId,
    ) -> Result<Game> {
        validate_slug(slug)?;
        validate_nonempty(name, "game name", 100)?;
        let chat_id = chat_id.0;
        let created_by = db_user_id(created_by)?;
        let mut transaction = self.pool.begin().await?;
        let has_default = query_scalar::<_, bool>(
            r"SELECT EXISTS(
    SELECT 1 FROM bingo_games
    WHERE chat_id = ? AND is_default = 1
) AS has_default",
        )
        .bind(chat_id)
        .fetch_one(&mut *transaction)
        .await?;
        let normalized_slug = slug.to_ascii_lowercase();
        let name = name.trim();
        let is_default = !has_default;
        let row = query_as::<_, GameRow>(
            r"INSERT INTO bingo_games (chat_id, slug, name, is_default, created_by)
VALUES (?, ?, ?, ?, ?)
RETURNING
id, chat_id, slug, name, description, center_text, state,
is_default",
        )
        .bind(chat_id)
        .bind(normalized_slug)
        .bind(name)
        .bind(is_default)
        .bind(created_by)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| map_unique(error, || format!("game `{slug}` already exists")))?;
        let game = row.try_into()?;
        transaction.commit().await?;
        Ok(game)
    }

    pub async fn list_games(&self, chat_id: ChatId) -> Result<Vec<Game>> {
        let chat_id = chat_id.0;
        let rows = query_as::<_, GameRow>(
            r"SELECT
    id, chat_id, slug, name, description, center_text, state,
    is_default
FROM bingo_games WHERE chat_id = ? ORDER BY id DESC",
        )
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(Game::try_from).collect()
    }

    pub async fn game(&self, chat_id: ChatId, slug: Option<&str>) -> Result<Game> {
        fetch_game(&self.pool, chat_id, slug).await
    }

    pub async fn set_game_state(
        &self,
        chat_id: ChatId,
        slug: &str,
        state: GameState,
    ) -> Result<Game> {
        let chat_id = chat_id.0;
        let row = query_as::<_, GameRow>(
            r"UPDATE bingo_games SET state = ?
WHERE chat_id = ? AND slug = ? COLLATE NOCASE
RETURNING
id, chat_id, slug, name, description, center_text, state,
is_default",
        )
        .bind(state.as_ref())
        .bind(chat_id)
        .bind(slug)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| BingoError::NotFound(format!("game `{slug}` was not found")))?;
        row.try_into()
    }

    pub async fn set_default_game(&self, chat_id: ChatId, slug: &str) -> Result<Game> {
        let mut transaction = self.pool.begin().await?;
        let game = fetch_game(&mut *transaction, chat_id, Some(slug)).await?;
        let chat_id = chat_id.0;
        query("UPDATE bingo_games SET is_default = 0 WHERE chat_id = ?")
            .bind(chat_id)
            .execute(&mut *transaction)
            .await?;
        let row = query_as::<_, GameRow>(
            r"UPDATE bingo_games SET is_default = 1 WHERE id = ?
RETURNING
id, chat_id, slug, name, description, center_text, state,
is_default",
        )
        .bind(game.id)
        .fetch_one(&mut *transaction)
        .await?;
        let game = row.try_into()?;
        transaction.commit().await?;
        Ok(game)
    }

    pub async fn set_center_text(&self, chat_id: ChatId, slug: &str, text: &str) -> Result<Game> {
        validate_nonempty(text, "center text", 60)?;
        let game = self.game(chat_id, Some(slug)).await?;
        ensure_editable(&game)?;
        let row = query_as::<_, GameRow>(
            r"UPDATE bingo_games SET center_text = ? WHERE id = ?
RETURNING
id, chat_id, slug, name, description, center_text, state,
is_default",
        )
        .bind(text.trim())
        .bind(game.id)
        .fetch_one(&self.pool)
        .await?;
        row.try_into()
    }

    pub async fn set_game_description(
        &self,
        chat_id: ChatId,
        slug: &str,
        description: &str,
    ) -> Result<Game> {
        let description = description.trim();
        if description.chars().count() > MAX_GAME_DESCRIPTION_CHARS {
            return Err(BingoError::InvalidCommand(format!(
                "game description must contain at most {MAX_GAME_DESCRIPTION_CHARS} characters"
            )));
        }
        let game = self.game(chat_id, Some(slug)).await?;
        ensure_editable(&game)?;
        let row = query_as::<_, GameRow>(
            r"UPDATE bingo_games SET description = ? WHERE id = ?
RETURNING
id, chat_id, slug, name, description, center_text, state,
is_default",
        )
        .bind(description)
        .bind(game.id)
        .fetch_one(&self.pool)
        .await?;
        row.try_into()
    }
}

impl TryFrom<GameRow> for Game {
    type Error = BingoError;

    fn try_from(row: GameRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            chat_id: ChatId(row.chat_id),
            slug: row.slug,
            name: row.name,
            description: row.description,
            center_text: row.center_text,
            state: row.state.parse()?,
            is_default: row.is_default,
        })
    }
}

async fn fetch_game<'e>(
    executor: impl SqliteExecutor<'e>,
    chat_id: ChatId,
    slug: Option<&str>,
) -> Result<Game> {
    let chat_id = chat_id.0;
    let row = query_as::<_, GameRow>(
        r"SELECT
    id, chat_id, slug, name, description, center_text, state,
    is_default
FROM bingo_games
WHERE chat_id = ?1
AND (slug = ?2 COLLATE NOCASE OR (?2 IS NULL AND is_default = 1))",
    )
    .bind(chat_id)
    .bind(slug)
    .fetch_optional(executor)
    .await?;
    row.map(Game::try_from).transpose()?.ok_or_else(|| {
        slug.map_or_else(
            || {
                BingoError::NotFound(
                    "this chat has no default bingo game; create one or set a default".to_owned(),
                )
            },
            |slug| BingoError::NotFound(format!("game `{slug}` was not found")),
        )
    })
}
