-- Known memory records (spec 2026-10-18-assistant-memory). The Markdown files stay the source; this
-- table remembers what PLA wrote so a line the user deleted is recognised as rejected.
CREATE TABLE memory_entry (
    id TEXT PRIMARY KEY,
    topic TEXT NOT NULL,
    role TEXT NOT NULL,
    kind TEXT NOT NULL,
    text TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('active', 'archived', 'rejected')),
    reason TEXT,
    changed_at TEXT NOT NULL
);
CREATE INDEX memory_entry_state ON memory_entry (state);
