CREATE TABLE media_posts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_id INTEGER NOT NULL,
    message_id INTEGER NOT NULL,
    sender_id INTEGER NOT NULL,
    sender_name TEXT NOT NULL,
    platform TEXT NOT NULL,
    cache_key TEXT NOT NULL,
    url TEXT NOT NULL,
    cache_hit INTEGER NOT NULL CHECK (cache_hit IN (0, 1)),
    created_at TEXT NOT NULL
);

CREATE INDEX media_posts_chat_created ON media_posts (chat_id, created_at);
