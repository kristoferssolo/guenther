use crate::bingo::{
    error::{BingoError, Result},
    model::{
        Card, CardCell, Entry, FREE_POSITION, Game, GameState, ImportedCell, KnownUser,
        MAX_ENTRY_CHARS, REQUIRED_ENTRIES, ToggleResult, has_bingo, normalize_entry,
    },
};
use rand::seq::SliceRandom;
use sqlx::{
    Sqlite, SqliteExecutor, SqlitePool, Transaction,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{env, path::Path, str::FromStr, time::Duration};

const DEFAULT_DATABASE_URL: &str = "sqlite://data/bingo.sqlite3";

#[derive(Debug, Clone)]
pub struct BingoStore {
    pool: SqlitePool,
}

#[derive(Debug)]
struct GameRow {
    id: i64,
    chat_id: i64,
    slug: String,
    name: String,
    center_text: String,
    state: String,
    is_default: bool,
}

#[derive(Debug)]
struct EntryRow {
    id: i64,
    game_id: i64,
    text: String,
}

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

impl BingoStore {
    pub async fn connect_from_env() -> Result<Self> {
        let database_url =
            env::var("BINGO_DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
        Self::connect(&database_url).await
    }

    pub async fn connect(database_url: &str) -> Result<Self> {
        create_database_parent(database_url).await?;
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5));
        let max_connections = if database_url.contains(":memory:") {
            1
        } else {
            5
        };
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn observe_user(&self, chat_id: i64, user: &KnownUser) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        if let Some(username) = &user.username {
            sqlx::query!(
                r#"UPDATE bingo_users SET username = NULL
WHERE chat_id = ? AND username = ? COLLATE NOCASE AND user_id != ?"#,
                chat_id,
                username,
                user.user_id,
            )
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query!(
            r#"INSERT INTO bingo_users (chat_id, user_id, username, display_name)
VALUES (?, ?, ?, ?) 
ON CONFLICT(chat_id, user_id) DO UPDATE SET username = excluded.username, 
display_name = excluded.display_name, updated_at = CURRENT_TIMESTAMP"#,
            chat_id,
            user.user_id,
            user.username,
            user.display_name,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn user_by_id(&self, chat_id: i64, user_id: i64) -> Result<KnownUser> {
        sqlx::query!(
            r#"SELECT user_id, username, display_name FROM bingo_users
WHERE chat_id = ? AND user_id = ?"#,
            chat_id,
            user_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .map(|row| KnownUser {
            user_id: row.user_id,
            username: row.username,
            display_name: row.display_name,
        })
        .ok_or_else(|| {
            BingoError::NotFound(format!("user `{user_id}` has not been seen in this chat"))
        })
    }

    pub async fn user_by_username(&self, chat_id: i64, username: &str) -> Result<KnownUser> {
        let username = username.trim_start_matches('@');
        sqlx::query!(
            r#"SELECT user_id, username, display_name FROM bingo_users
WHERE chat_id = ? AND username = ? COLLATE NOCASE"#,
            chat_id,
            username,
        )
        .fetch_optional(&self.pool)
        .await?
        .map(|row| KnownUser {
            user_id: row.user_id,
            username: row.username,
            display_name: row.display_name,
        })
        .ok_or_else(|| {
            BingoError::NotFound(format!(
                "@{username} has not been seen in this chat; reply to one of their messages instead"
            ))
        })
    }

    pub async fn create_game(
        &self,
        chat_id: i64,
        slug: &str,
        name: &str,
        created_by: i64,
    ) -> Result<Game> {
        validate_slug(slug)?;
        validate_nonempty(name, "game name", 100)?;
        let mut transaction = self.pool.begin().await?;
        let has_default = sqlx::query_scalar!(
            r#"SELECT EXISTS(
    SELECT 1 FROM bingo_games
    WHERE chat_id = ? AND is_default = 1
) AS `has_default!: bool`"#,
            chat_id,
        )
        .fetch_one(&mut *transaction)
        .await?;
        let normalized_slug = slug.to_ascii_lowercase();
        let name = name.trim();
        sqlx::query!(
            r#"INSERT INTO bingo_games (chat_id, slug, name, is_default, created_by)
VALUES (?, ?, ?, ?, ?)"#,
            chat_id,
            normalized_slug,
            name,
            !has_default,
            created_by,
        )
        .execute(&mut *transaction)
        .await
        .map_err(|error| map_unique(error, format!("game `{slug}` already exists")))?;
        let game = fetch_game(&mut *transaction, chat_id, Some(slug)).await?;
        transaction.commit().await?;
        Ok(game)
    }

    pub async fn list_games(&self, chat_id: i64) -> Result<Vec<Game>> {
        let rows = sqlx::query_as!(
            GameRow,
            r#"SELECT
    id, chat_id, slug, name, center_text, state,
    is_default AS `is_default: bool`
FROM bingo_games WHERE chat_id = ? ORDER BY id DESC"#,
            chat_id,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(Game::try_from).collect()
    }

    pub async fn game(&self, chat_id: i64, slug: Option<&str>) -> Result<Game> {
        fetch_game(&self.pool, chat_id, slug).await
    }

    pub async fn set_game_state(&self, chat_id: i64, slug: &str, state: GameState) -> Result<Game> {
        let result = sqlx::query!(
            r#"UPDATE bingo_games SET state = ?
WHERE chat_id = ? AND slug = ? COLLATE NOCASE"#,
            state.as_str(),
            chat_id,
            slug,
        )
        .execute(&self.pool)
        .await?;
        require_changed(
            result.rows_affected(),
            format!("game `{slug}` was not found"),
        )?;
        self.game(chat_id, Some(slug)).await
    }

    pub async fn set_default_game(&self, chat_id: i64, slug: &str) -> Result<Game> {
        let mut transaction = self.pool.begin().await?;
        let game = fetch_game(&mut *transaction, chat_id, Some(slug)).await?;
        sqlx::query!(
            r#"UPDATE bingo_games SET is_default = 0 WHERE chat_id = ?"#,
            chat_id,
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query!(
            r#"UPDATE bingo_games SET is_default = 1 WHERE id = ?"#,
            game.id,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.game(chat_id, Some(slug)).await
    }

    pub async fn set_center_text(&self, chat_id: i64, slug: &str, text: &str) -> Result<Game> {
        validate_nonempty(text, "center text", 60)?;
        let game = self.game(chat_id, Some(slug)).await?;
        ensure_editable(&game)?;
        sqlx::query!(
            r#"UPDATE bingo_games SET center_text = ? WHERE id = ?"#,
            text.trim(),
            game.id,
        )
        .execute(&self.pool)
        .await?;
        self.game(chat_id, Some(slug)).await
    }

    pub async fn add_entry(&self, chat_id: i64, slug: Option<&str>, text: &str) -> Result<Entry> {
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
        chat_id: i64,
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

    pub async fn edit_entry(&self, chat_id: i64, entry_id: i64, text: &str) -> Result<Entry> {
        validate_nonempty(text, "entry", MAX_ENTRY_CHARS)?;
        let normalized = normalize_entry(text);
        let result = sqlx::query!(
            r#"UPDATE bingo_entries SET text = ?, normalized_text = ?
WHERE id = ? AND game_id IN
(SELECT id FROM bingo_games WHERE chat_id = ? AND state != 'closed') AND active = 1"#,
            text.trim(),
            normalized,
            entry_id,
            chat_id,
        )
        .execute(&self.pool)
        .await
        .map_err(|error| map_unique(error, "that entry already exists".to_owned()))?;
        require_changed(
            result.rows_affected(),
            format!("entry `{entry_id}` was not found"),
        )?;
        let row = sqlx::query_as!(
            EntryRow,
            r#"SELECT id, game_id, text FROM bingo_entries WHERE id = ?"#,
            entry_id,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.into())
    }

    pub async fn delete_entry(&self, chat_id: i64, entry_id: i64) -> Result<()> {
        let result = sqlx::query!(
            r#"UPDATE bingo_entries SET active = 0
WHERE id = ? AND game_id IN
(SELECT id FROM bingo_games WHERE chat_id = ? AND state != 'closed') AND active = 1"#,
            entry_id,
            chat_id,
        )
        .execute(&self.pool)
        .await?;
        require_changed(
            result.rows_affected(),
            format!("entry `{entry_id}` was not found"),
        )
    }

    pub async fn generate_card(
        &self,
        chat_id: i64,
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
        let mut entry_index = 0;
        for position in 0..25 {
            if position == FREE_POSITION {
                insert_cell(
                    &mut transaction,
                    card_id,
                    position,
                    None,
                    &game.center_text,
                    true,
                    true,
                )
                .await?;
            } else {
                let entry = &entries[entry_index];
                insert_cell(
                    &mut transaction,
                    card_id,
                    position,
                    Some(entry.id),
                    &entry.text,
                    false,
                    false,
                )
                .await?;
                entry_index += 1;
            }
        }
        transaction.commit().await?;
        self.card_by_id(card_id).await
    }

    pub async fn import_card(
        &self,
        chat_id: i64,
        slug: &str,
        owner: &KnownUser,
        imported: &[ImportedCell],
        replace: bool,
    ) -> Result<Card> {
        if imported.len() != 25 {
            return Err(BingoError::InvalidCommand(
                "an imported card must contain 25 cells".to_owned(),
            ));
        }
        let game = self.game(chat_id, Some(slug)).await?;
        ensure_editable(&game)?;
        let mut transaction = self.pool.begin().await?;
        delete_or_reject_existing(&mut transaction, game.id, owner.user_id, replace).await?;
        let card_id = insert_card(&mut transaction, game.id, owner).await?;
        for (position, cell) in imported.iter().enumerate() {
            if position == FREE_POSITION {
                insert_cell(
                    &mut transaction,
                    card_id,
                    position,
                    None,
                    &game.center_text,
                    true,
                    true,
                )
                .await?;
                continue;
            }
            let entry_id = upsert_entry(&mut *transaction, game.id, &cell.text).await?;
            insert_cell(
                &mut transaction,
                card_id,
                position,
                Some(entry_id),
                &cell.text,
                cell.marked,
                false,
            )
            .await?;
        }
        transaction.commit().await?;
        self.card_by_id(card_id).await
    }

    pub async fn card(&self, chat_id: i64, slug: Option<&str>, user_id: i64) -> Result<Card> {
        let game = self.game(chat_id, slug).await?;
        let card_id = sqlx::query_scalar!(
            r#"SELECT id FROM bingo_cards WHERE game_id = ? AND user_id = ?"#,
            game.id,
            user_id,
        )
        .fetch_optional(&self.pool)
        .await?;
        self.card_by_id(card_id.ok_or_else(|| {
            BingoError::NotFound(format!(
                "no card is assigned to that user for `{}`",
                game.slug
            ))
        })?)
        .await
    }

    pub async fn set_card_cell(
        &self,
        chat_id: i64,
        slug: &str,
        user_id: i64,
        position: usize,
        text: &str,
    ) -> Result<Card> {
        if position == FREE_POSITION {
            return Err(BingoError::FreeCell);
        }
        validate_nonempty(text, "cell", MAX_ENTRY_CHARS)?;
        let game = self.game(chat_id, Some(slug)).await?;
        ensure_editable(&game)?;
        let mut transaction = self.pool.begin().await?;
        let card_id = card_id_in(&mut transaction, game.id, user_id).await?;
        let entry_id = upsert_entry(&mut *transaction, game.id, text).await?;
        let position = i64::try_from(position)
            .map_err(|_| BingoError::InvalidCommand("invalid cell position".to_owned()))?;
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
        self.card_by_id(card_id).await
    }

    pub async fn reset_card(&self, chat_id: i64, slug: Option<&str>, user_id: i64) -> Result<Card> {
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
        self.card_by_id(card_id).await
    }

    pub async fn toggle_cell(
        &self,
        card_id: i64,
        user_id: i64,
        position: usize,
    ) -> Result<ToggleResult> {
        if position == FREE_POSITION {
            return Err(BingoError::FreeCell);
        }
        let position = i64::try_from(position)
            .map_err(|_| BingoError::InvalidCommand("invalid cell position".to_owned()))?;
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
        if row.user_id != user_id {
            return Err(BingoError::NotCardOwner);
        }
        if GameState::try_from(row.state.as_str())? != GameState::Active {
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
        require_changed(
            result.rows_affected(),
            "that card cell was not found".to_owned(),
        )?;
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
        let card = self.card_by_id(card_id).await?;
        Ok(ToggleResult {
            card,
            newly_completed,
        })
    }

    async fn card_by_id(&self, card_id: i64) -> Result<Card> {
        let row = sqlx::query_as!(
            CardRow,
            r#"SELECT
    c.id AS card_id, c.user_id, c.owner_name,
    c.bingo_announced AS `bingo_announced: bool`, 
    g.id AS game_id, g.chat_id, g.slug, g.name AS game_name, g.center_text, 
    g.state, g.is_default AS `is_default: bool`
FROM bingo_cards c JOIN bingo_games g ON g.id = c.game_id WHERE c.id = ?"#,
            card_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| BingoError::NotFound("that bingo card no longer exists".to_owned()))?;
        let user = sqlx::query!(
            r#"SELECT user_id, username, display_name FROM bingo_users
WHERE chat_id = ? AND user_id = ?"#,
            row.chat_id,
            row.user_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .map_or_else(
            || KnownUser {
                user_id: row.user_id,
                username: None,
                display_name: row.owner_name.clone(),
            },
            |known| KnownUser {
                user_id: known.user_id,
                username: known.username,
                display_name: known.display_name,
            },
        );
        let cells = fetch_cells(&self.pool, card_id).await?;
        Ok(Card {
            id: row.card_id,
            game: Game {
                id: row.game_id,
                chat_id: row.chat_id,
                slug: row.slug,
                name: row.game_name,
                center_text: row.center_text,
                state: GameState::try_from(row.state.as_str())?,
                is_default: row.is_default,
            },
            owner: user,
            bingo_announced: row.bingo_announced,
            cells,
        })
    }
}

impl TryFrom<GameRow> for Game {
    type Error = BingoError;

    fn try_from(row: GameRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            chat_id: row.chat_id,
            slug: row.slug,
            name: row.name,
            center_text: row.center_text,
            state: GameState::try_from(row.state.as_str())?,
            is_default: row.is_default,
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

async fn create_database_parent(database_url: &str) -> Result<()> {
    let Some(path) = database_url.strip_prefix("sqlite://") else {
        return Ok(());
    };
    if path == ":memory:" || path.starts_with("file:") {
        return Ok(());
    }
    let path = Path::new(path.split('?').next().unwrap_or(path));
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(())
}

async fn fetch_game<'e>(
    executor: impl SqliteExecutor<'e>,
    chat_id: i64,
    slug: Option<&str>,
) -> Result<Game> {
    let row = sqlx::query_as!(
        GameRow,
        r#"SELECT
    id, chat_id, slug, name, center_text, state,
    is_default AS `is_default: bool`
FROM bingo_games
WHERE chat_id = ?1
  AND (slug = ?2 COLLATE NOCASE OR (?2 IS NULL AND is_default = 1))"#,
        chat_id,
        slug,
    )
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

async fn delete_or_reject_existing(
    transaction: &mut Transaction<'_, Sqlite>,
    game_id: i64,
    user_id: i64,
    replace: bool,
) -> Result<()> {
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

async fn insert_card(
    transaction: &mut Transaction<'_, Sqlite>,
    game_id: i64,
    owner: &KnownUser,
) -> Result<i64> {
    let owner_name = owner.label();
    let result = sqlx::query!(
        r#"INSERT INTO bingo_cards (game_id, user_id, owner_name) VALUES (?, ?, ?)"#,
        game_id,
        owner.user_id,
        owner_name,
    )
    .execute(&mut **transaction)
    .await?;
    Ok(result.last_insert_rowid())
}

#[allow(clippy::too_many_arguments)]
async fn insert_cell(
    transaction: &mut Transaction<'_, Sqlite>,
    card_id: i64,
    position: usize,
    entry_id: Option<i64>,
    text: &str,
    marked: bool,
    is_free: bool,
) -> Result<()> {
    let position = i64::try_from(position)
        .map_err(|_| BingoError::InvalidCommand("invalid cell position".to_owned()))?;
    sqlx::query!(
        r#"INSERT INTO bingo_card_cells
(card_id, position, entry_id, text, marked, is_free) VALUES (?, ?, ?, ?, ?, ?)"#,
        card_id,
        position,
        entry_id,
        text.trim(),
        marked,
        is_free,
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn upsert_entry<'e>(
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

async fn fetch_cells<'e>(executor: impl SqliteExecutor<'e>, card_id: i64) -> Result<Vec<CardCell>> {
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

async fn card_id_in(
    transaction: &mut Transaction<'_, Sqlite>,
    game_id: i64,
    user_id: i64,
) -> Result<i64> {
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
            let position = usize::try_from(row.position).map_err(|_| {
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

fn validate_slug(slug: &str) -> Result<()> {
    let valid = (2..=50).contains(&slug.len())
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(BingoError::InvalidCommand(
            "game slugs must be 2-50 lowercase letters, numbers, or hyphens".to_owned(),
        ))
    }
}

fn validate_nonempty(text: &str, label: &str, max_chars: usize) -> Result<()> {
    let length = text.trim().chars().count();
    if length == 0 || length > max_chars {
        Err(BingoError::InvalidCommand(format!(
            "{label} must contain between 1 and {max_chars} characters"
        )))
    } else {
        Ok(())
    }
}

fn ensure_active(game: &Game) -> Result<()> {
    if game.state == GameState::Active {
        Ok(())
    } else {
        Err(BingoError::GameNotActive)
    }
}

fn ensure_editable(game: &Game) -> Result<()> {
    if game.state == GameState::Closed {
        Err(BingoError::Conflict(
            "closed games cannot be changed".to_owned(),
        ))
    } else {
        Ok(())
    }
}

fn require_changed(rows_affected: u64, message: String) -> Result<()> {
    if rows_affected == 0 {
        Err(BingoError::NotFound(message))
    } else {
        Ok(())
    }
}

fn map_unique(error: sqlx::Error, message: String) -> BingoError {
    if matches!(&error, sqlx::Error::Database(database) if database.is_unique_violation()) {
        BingoError::Conflict(message)
    } else {
        BingoError::Database(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};

    fn user(id: i64, username: &str) -> KnownUser {
        KnownUser {
            user_id: id,
            username: Some(username.to_owned()),
            display_name: username.to_owned(),
        }
    }

    async fn store() -> BingoStore {
        BingoStore::connect("sqlite::memory:")
            .await
            .expect("create in-memory bingo store")
    }

    #[tokio::test]
    async fn generates_persistent_card_with_free_center() {
        let store = store().await;
        let owner = user(10, "driver");
        assert_ok!(store.observe_user(1, &owner).await);
        assert_ok!(store.create_game(1, "season", "Season", 10).await);
        assert_ok!(store.set_game_state(1, "season", GameState::Active).await);
        for index in 0..REQUIRED_ENTRIES {
            assert_ok!(store.add_entry(1, None, &format!("Entry {index}")).await);
        }
        let card = store
            .generate_card(1, None, &owner, false)
            .await
            .expect("generate card");
        assert_eq!(card.cells.len(), 25);
        assert!(card.cells[FREE_POSITION].is_free);
        assert!(card.cells[FREE_POSITION].marked);
        let fetched = store
            .card(1, None, owner.user_id)
            .await
            .expect("fetch card");
        assert_eq!(fetched.cells, card.cells);
    }

    #[tokio::test]
    async fn rejects_duplicate_generation_without_replace() {
        let store = store().await;
        let owner = user(10, "driver");
        assert_ok!(store.observe_user(1, &owner).await);
        assert_ok!(store.create_game(1, "season", "Season", 10).await);
        assert_ok!(store.set_game_state(1, "season", GameState::Active).await);
        for index in 0..REQUIRED_ENTRIES {
            assert_ok!(store.add_entry(1, None, &format!("Entry {index}")).await);
        }
        assert_ok!(store.generate_card(1, None, &owner, false).await);
        assert_err!(store.generate_card(1, None, &owner, false).await);
        assert_ok!(store.generate_card(1, None, &owner, true).await);
    }

    #[tokio::test]
    async fn only_owner_can_mark_and_first_line_is_announced_once() {
        let store = store().await;
        let owner = user(10, "driver");
        assert_ok!(store.observe_user(1, &owner).await);
        assert_ok!(store.create_game(1, "season", "Season", 10).await);
        assert_ok!(store.set_game_state(1, "season", GameState::Active).await);
        for index in 0..REQUIRED_ENTRIES {
            assert_ok!(store.add_entry(1, None, &format!("Entry {index}")).await);
        }
        let card = store
            .generate_card(1, None, &owner, false)
            .await
            .expect("generate card");

        assert_err!(store.toggle_cell(card.id, 99, 0).await);
        for position in 0..4 {
            let toggle = store
                .toggle_cell(card.id, owner.user_id, position)
                .await
                .expect("mark cell");
            assert!(!toggle.newly_completed);
        }
        let toggle = store
            .toggle_cell(card.id, owner.user_id, 4)
            .await
            .expect("complete row");
        assert!(toggle.newly_completed);
        assert!(toggle.card.has_bingo());

        assert_ok!(store.toggle_cell(card.id, owner.user_id, 4).await);
        let repeated = store
            .toggle_cell(card.id, owner.user_id, 4)
            .await
            .expect("complete same row again");
        assert!(!repeated.newly_completed);
    }

    #[tokio::test]
    async fn edits_do_not_change_existing_card_snapshots() {
        let store = store().await;
        let owner = user(10, "driver");
        assert_ok!(store.observe_user(1, &owner).await);
        assert_ok!(store.create_game(1, "season", "Season", 10).await);
        assert_ok!(store.set_game_state(1, "season", GameState::Active).await);
        for index in 0..REQUIRED_ENTRIES {
            assert_ok!(store.add_entry(1, None, &format!("Entry {index}")).await);
        }
        let card = store
            .generate_card(1, None, &owner, false)
            .await
            .expect("generate card");
        let original_texts = card
            .cells
            .iter()
            .map(|cell| cell.text.clone())
            .collect::<Vec<_>>();
        let (_, entries) = store.list_entries(1, None).await.expect("list entries");
        assert_ok!(store.edit_entry(1, entries[0].id, "Changed entry").await);

        let fetched = store
            .card(1, None, owner.user_id)
            .await
            .expect("fetch card");
        assert_eq!(
            fetched
                .cells
                .iter()
                .map(|cell| cell.text.clone())
                .collect::<Vec<_>>(),
            original_texts
        );
    }
}
