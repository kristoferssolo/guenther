use crate::bingo::error::Result;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use teloxide::{
    prelude::{Bot, ChatId, Requester},
    types::{Message, UserId},
};
use tokio::sync::Mutex;

const ADMIN_CACHE_TTL: Duration = Duration::from_mins(1);

#[derive(Debug, Clone)]
pub struct AdminCache {
    entries: Arc<Mutex<HashMap<ChatId, CachedAdministrators>>>,
    ttl: Duration,
}

#[derive(Debug)]
struct CachedAdministrators {
    expires_at: Instant,
    user_ids: HashSet<UserId>,
}

impl Default for AdminCache {
    fn default() -> Self {
        Self {
            entries: Arc::default(),
            ttl: ADMIN_CACHE_TTL,
        }
    }
}

impl AdminCache {
    async fn status(&self, chat_id: ChatId, user_id: UserId) -> Option<bool> {
        let mut entries = self.entries.lock().await;
        if entries
            .get(&chat_id)
            .is_some_and(|entry| entry.expires_at <= Instant::now())
        {
            entries.remove(&chat_id);
            return None;
        }
        entries
            .get(&chat_id)
            .map(|entry| entry.user_ids.contains(&user_id))
    }

    async fn insert(&self, chat_id: ChatId, user_ids: HashSet<UserId>) {
        self.entries.lock().await.insert(
            chat_id,
            CachedAdministrators {
                expires_at: Instant::now() + self.ttl,
                user_ids,
            },
        );
    }

    #[cfg(test)]
    fn with_ttl(ttl: Duration) -> Self {
        Self {
            entries: Arc::default(),
            ttl,
        }
    }
}

pub async fn is_chat_admin(bot: &Bot, message: &Message, cache: &AdminCache) -> Result<bool> {
    let Some(user) = message.from.as_ref() else {
        return Ok(false);
    };
    if message.chat.is_private() {
        return Ok(true);
    }
    if let Some(is_admin) = cache.status(message.chat.id, user.id).await {
        return Ok(is_admin);
    }
    let administrators = bot.get_chat_administrators(message.chat.id).await?;
    let user_ids = administrators
        .into_iter()
        .map(|member| member.user.id)
        .collect::<HashSet<_>>();
    let is_admin = user_ids.contains(&user.id);
    cache.insert(message.chat.id, user_ids).await;
    Ok(is_admin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_some_eq};

    #[tokio::test]
    async fn administrator_cache_respects_expiration() {
        let chat_id = ChatId(1);
        let user_id = UserId(2);
        let cache = AdminCache::with_ttl(Duration::from_mins(1));
        cache.insert(chat_id, HashSet::from([user_id])).await;
        assert_some_eq!(cache.status(chat_id, user_id).await, true);
        assert_some_eq!(cache.status(chat_id, UserId(3)).await, false);

        let expired = AdminCache::with_ttl(Duration::ZERO);
        expired.insert(chat_id, HashSet::from([user_id])).await;
        assert_none!(expired.status(chat_id, user_id).await);
    }
}
