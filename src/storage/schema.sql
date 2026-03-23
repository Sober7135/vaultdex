PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS notes (
    id               INTEGER PRIMARY KEY,
    path             TEXT NOT NULL UNIQUE,
    mtime            INTEGER,
    content_hash     TEXT NOT NULL,
    index_at         INTEGER NOT NULL,
    frontmatter_json TEXT,
    content          TEXT NOT NULL,
    word_count       INTEGER NOT NULL,
    line_count       INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS note_headings (
    id                INTEGER PRIMARY KEY,
    note_id           INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    heading_path_json TEXT NOT NULL,
    level             INTEGER NOT NULL,
    title             TEXT NOT NULL,
    normalized_text   TEXT NOT NULL,
    start_line        INTEGER NOT NULL,
    end_line          INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_note_headings_note_id
    ON note_headings(note_id);

CREATE TABLE IF NOT EXISTS links (
    id             INTEGER PRIMARY KEY,
    note_id        INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    raw            TEXT NOT NULL,
    target_note    TEXT NOT NULL,
    target_heading TEXT,
    alias          TEXT,
    is_embed       INTEGER NOT NULL,
    line           INTEGER NOT NULL,
    byte_start     INTEGER NOT NULL,
    byte_end       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_links_note_id
    ON links(note_id);

CREATE INDEX IF NOT EXISTS idx_links_target_note
    ON links(target_note);

CREATE TABLE IF NOT EXISTS tags (
    id         INTEGER PRIMARY KEY,
    note_id    INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    raw        TEXT NOT NULL,
    normalized TEXT NOT NULL,
    source     TEXT NOT NULL,
    line       INTEGER
);

CREATE INDEX IF NOT EXISTS idx_tags_note_id
    ON tags(note_id);

CREATE INDEX IF NOT EXISTS idx_tags_normalized
    ON tags(normalized);
