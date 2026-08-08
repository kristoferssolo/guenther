use super::BingoStore;
use crate::bingo::{
    error::{BingoError, Result},
    model::KnownUser,
};

impl BingoStore {
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
}
