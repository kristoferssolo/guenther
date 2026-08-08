mod card;
mod connection;
mod entry;
mod game;
mod id;
mod user;
mod validation;

use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct BingoStore {
    pool: SqlitePool,
}

#[cfg(test)]
mod tests;
