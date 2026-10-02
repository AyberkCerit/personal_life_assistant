-- Block text is kept for fuzzy re-matching (FR-EXT-006); the note stays the source of truth.
ALTER TABLE block ADD COLUMN text TEXT NOT NULL DEFAULT '';
-- Hash of the text that was last sent to the model; differs from text_hash => needs extraction.
ALTER TABLE block ADD COLUMN extracted_hash TEXT;

-- Notes waiting for extraction; survives restarts and model outages (FR-EXT-022).
CREATE TABLE extraction_queue (
  note_path  TEXT PRIMARY KEY,
  queued_at  TEXT NOT NULL,
  attempts   INTEGER NOT NULL DEFAULT 0,
  last_error TEXT
);
