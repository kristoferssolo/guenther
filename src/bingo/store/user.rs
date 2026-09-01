use crate::bingo::{
    error::{BingoError, Result},
    model::KnownUser,
    store::{
        BingoStore,
        id::{db_user_id, user_id_from_db},
    },
};
use teloxide::types::{ChatId, UserId};

impl BingoStore {
    pub async fn observe_user(&self, chat_id: ChatId, user: &KnownUser) -> Result<()> {
        let chat_id = chat_id.0;
        let user_id = db_user_id(user.user_id)?;
        let mut transaction = self.pool.begin().await?;
        if let Some(username) = &user.username {
            sqlx::query!(
                r#"
                UPDATE
                    bingo_users
                SET
                    username = NULL
                WHERE
                    chat_id = ?
                    AND username = ? COLLATE NOCASE
                    AND user_id != ?
                "#,
                chat_id,
                username,
                user_id,
            )
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query!(
            r#"
            INSERT INTO
                bingo_users (chat_id, user_id, username, display_name)
            VALUES
                (?, ?, ?, ?) ON CONFLICT(chat_id, user_id) DO
            UPDATE
            SET
                username = excluded.username,
                display_name = excluded.display_name,
                updated_at = CURRENT_TIMESTAMP
            "#,
            chat_id,
            user_id,
            user.username,
            user.display_name,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn user_by_id(&self, chat_id: ChatId, user_id: UserId) -> Result<KnownUser> {
        let chat_id = chat_id.0;
        let db_user_id = db_user_id(user_id)?;
        sqlx::query!(
            r#"
            SELECT
                user_id,
                username,
                display_name
            FROM
                bingo_users
            WHERE
                chat_id = ?
                AND user_id = ?
            "#,
            chat_id,
            db_user_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .map(|row| -> Result<KnownUser> {
            Ok(KnownUser {
                user_id: user_id_from_db(row.user_id)?,
                username: row.username,
                display_name: row.display_name,
            })
        })
        .transpose()?
        .ok_or_else(|| {
            BingoError::NotFound(format!("User `{user_id}` has not been seen in this chat"))
        })
    }

    pub async fn user_by_username(&self, chat_id: ChatId, username: &str) -> Result<KnownUser> {
        let username = username.trim_start_matches('@');
        let chat_id = chat_id.0;
        sqlx::query!(
            r#"
            SELECT
                user_id,
                username,
                display_name
            FROM
                bingo_users
            WHERE
                chat_id = ?
                AND username = ? COLLATE NOCASE
            "#,
            chat_id,
            username,
        )
        .fetch_optional(&self.pool)
        .await?
        .map(|row| -> Result<KnownUser> {
            Ok(KnownUser {
                user_id: user_id_from_db(row.user_id)?,
                username: row.username,
                display_name: row.display_name,
            })
        })
        .transpose()?
        .ok_or_else(|| {
            BingoError::NotFound(format!(
                "User `@{username}` has not been seen in this chat; reply to one of their messages instead"
            ))
        })
    }
}
