CREATE TABLE bingo_users (
    chat_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    username TEXT COLLATE nocase,
    display_name TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (chat_id, user_id)
);

CREATE UNIQUE INDEX bingo_users_chat_username
ON bingo_users (chat_id, username)
WHERE username IS NOT NULL;

CREATE TABLE bingo_games (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_id INTEGER NOT NULL,
    slug TEXT NOT NULL COLLATE nocase,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    center_text TEXT NOT NULL DEFAULT 'LIGHTS OUT!',
    state TEXT NOT NULL DEFAULT 'draft'
    CHECK (state IN ('draft', 'active', 'closed')),
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_by INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (chat_id, slug)
);

CREATE UNIQUE INDEX bingo_games_one_default
ON bingo_games (chat_id)
WHERE is_default = 1;

CREATE TABLE bingo_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id INTEGER NOT NULL REFERENCES bingo_games(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    normalized_text TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (game_id, normalized_text)
);

CREATE INDEX bingo_entries_game_active
ON bingo_entries (game_id, active);

CREATE TABLE bingo_cards (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id INTEGER NOT NULL REFERENCES bingo_games(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL,
    owner_name TEXT NOT NULL,
    bingo_announced INTEGER NOT NULL DEFAULT 0
    CHECK (bingo_announced IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (game_id, user_id)
);

CREATE TABLE bingo_card_cells (
    card_id INTEGER NOT NULL REFERENCES bingo_cards(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position BETWEEN 0 AND 24),
    entry_id INTEGER REFERENCES bingo_entries(id) ON DELETE SET NULL,
    text TEXT NOT NULL,
    marked INTEGER NOT NULL DEFAULT 0 CHECK (marked IN (0, 1)),
    is_free INTEGER NOT NULL DEFAULT 0 CHECK (is_free IN (0, 1)),
    PRIMARY KEY (card_id, position)
);

CREATE INDEX bingo_cards_game_user ON bingo_cards (game_id, user_id);
