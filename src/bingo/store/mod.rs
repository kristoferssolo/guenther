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

impl BingoStore {
    pub const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[cfg(test)]
mod tests;
