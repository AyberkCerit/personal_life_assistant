-- FR-MEM-001…005: chunks with what they were embedded with, their keyword index and their vectors
-- (sqlite-vec, cosine). Appendix C's placeholder table is replaced; the cache rebuilds from the vault.
DROP TABLE chunk;
CREATE TABLE chunk (
  chunk_id       INTEGER PRIMARY KEY,
  note_path      TEXT NOT NULL,
  ord            INTEGER NOT NULL,
  heading        TEXT NOT NULL DEFAULT '',
  text           TEXT NOT NULL,
  token_count    INTEGER NOT NULL,
  text_hash      TEXT NOT NULL,
  embedded_model TEXT
);
CREATE INDEX chunk_note ON chunk(note_path);
CREATE INDEX chunk_model ON chunk(embedded_model);
CREATE VIRTUAL TABLE chunk_fts USING fts5(title, text);
CREATE VIRTUAL TABLE chunk_vec USING vec0(chunk_id INTEGER PRIMARY KEY, embedding float[768] distance_metric=cosine);
DELETE FROM note_index;
DELETE FROM note_fts;
DELETE FROM link;
