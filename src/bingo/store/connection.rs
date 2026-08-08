use crate::bingo::{error::Result, store::BingoStore};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::{env, path::Path, str::FromStr, time::Duration};

const DEFAULT_DATABASE_URL: &str = "sqlite://data/bingo.sqlite3";

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
