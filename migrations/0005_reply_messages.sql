-- Messages of in-character reply threads. The Bot API only exposes the
-- immediate parent of a reply, so the bot keeps the chain itself.
CREATE TABLE reply_messages (
    chat_id INTEGER NOT NULL,
    message_id INTEGER NOT NULL,
    -- No foreign key: pruning removes old parents before their replies.
    parent_message_id INTEGER,
    -- NULL for Guenther's own messages.
    member_name TEXT,
    text TEXT,
    -- Media of the message that opened the thread, repeated on each of
    -- Guenther's messages so a reply to any of them can fetch it.
    media_kind TEXT CHECK (media_kind IN ('video', 'image')),
    media_file_id TEXT,
    created_at TEXT NOT NULL,
    PRIMARY KEY (chat_id, message_id),
    CHECK ((media_kind IS NULL) = (media_file_id IS NULL))
);

CREATE INDEX reply_messages_created ON reply_messages (created_at);
