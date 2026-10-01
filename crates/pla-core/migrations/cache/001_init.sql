CREATE TABLE note_index (
  note_path    TEXT PRIMARY KEY,
  title        TEXT,
  aliases      TEXT,
  tags         TEXT,
  mtime        INTEGER NOT NULL,
  size         INTEGER NOT NULL,
  content_hash TEXT NOT NULL
);

CREATE VIRTUAL TABLE note_fts USING fts5(note_path UNINDEXED, title, body);

CREATE TABLE link (
  source_path TEXT NOT NULL,
  target_path TEXT NOT NULL,
  line_text   TEXT
);
CREATE INDEX link_target ON link(target_path);

CREATE TABLE chunk (
  chunk_id        INTEGER PRIMARY KEY,
  note_path       TEXT NOT NULL,
  start_offset    INTEGER NOT NULL,
  text            TEXT NOT NULL,
  token_count     INTEGER NOT NULL,
  needs_embedding INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX chunk_note ON chunk(note_path);

CREATE TABLE daily_summary (
  date              TEXT PRIMARY KEY,
  summary_text      TEXT NOT NULL,
  source_paths_json TEXT NOT NULL,
  generated_at      TEXT NOT NULL,
  stale             INTEGER NOT NULL DEFAULT 0
);
