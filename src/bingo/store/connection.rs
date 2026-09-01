use super::BingoStore;

#[cfg(test)]
use crate::bingo::error::Result;
#[cfg(test)]
use guenther::db;

impl BingoStore {
    /// Connect to the shared database and apply all embedded migrations.
    ///
    /// Only tests use this directly; `main` connects once and shares the pool.
    #[cfg(test)]
    pub async fn connect(database_url: &str) -> Result<Self> {
        Ok(Self::new(db::connect(database_url).await?))
    }
}
