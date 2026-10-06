-- M3: where a wikilink stands, so a backlink can show and select its line (FR-EDT-011).
ALTER TABLE link ADD COLUMN line INTEGER NOT NULL DEFAULT 0;
