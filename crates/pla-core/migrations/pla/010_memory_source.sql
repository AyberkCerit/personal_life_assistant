-- Notes the State inference has read (spec 2026-10-18-assistant-memory, "Notlardan"): the content
-- hash it read, so only a note changed since is read again, and a note gone from the index drops
-- its inferences.
CREATE TABLE memory_source (
    note_path TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    read_at TEXT NOT NULL
);
