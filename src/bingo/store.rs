mod card;
mod connection;
mod entry;
mod game;
mod user;
mod validation;

#[cfg(test)]
mod tests;

use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct BingoStore {
    pool: SqlitePool,
}
