use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{env, path::Path, str::FromStr, time::Duration};
use thiserror::Error;

pub const DEFAULT_DATABASE_URL: &str = "sqlite://data/bingo.sqlite3";

#[derive(Debug, Error)]
pub enum DbError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Database migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("Filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DbError>;

/// Connect to the SQLite database and apply all embedded migrations.
///
/// The parent directory of the database file is created if missing, so the
/// default `sqlite://data/bingo.sqlite3` URL works on a fresh checkout.
///
/// # Errors
///
/// Returns an error when the database cannot be opened or migrated.
pub async fn connect(database_url: &str) -> Result<SqlitePool> {
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
    Ok(pool)
}

/// Connect using `BINGO_DATABASE_URL`, falling back to the default location.
///
/// # Errors
///
/// Returns an error when the database cannot be opened or migrated.
pub async fn connect_from_env() -> Result<SqlitePool> {
    let database_url =
        env::var("BINGO_DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
    connect(&database_url).await
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
