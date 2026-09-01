CREATE TABLE media_cache (
    url      TEXT    NOT NULL,
    position INTEGER NOT NULL,
    kind     TEXT    NOT NULL CHECK (kind IN ('video', 'image')),
    file_id  TEXT    NOT NULL,
    PRIMARY KEY (url, position)
);
