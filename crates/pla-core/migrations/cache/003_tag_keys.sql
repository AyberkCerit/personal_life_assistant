-- links final review I2: tag filters match Turkish i/ı alike; the cache is rebuilt with the new keys.
ALTER TABLE note_index ADD COLUMN tag_keys TEXT NOT NULL DEFAULT '[]';
DELETE FROM note_index;
DELETE FROM note_fts;
DELETE FROM link;
