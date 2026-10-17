-- Weekly routines (spec 2026-10-18): a routine repeats on chosen weekdays; each due day is one task row.
CREATE TABLE routine (
  routine_id     TEXT PRIMARY KEY,
  title          TEXT NOT NULL CHECK (length(title) <= 200),
  days_json      TEXT NOT NULL,
  paused         INTEGER NOT NULL DEFAULT 0,
  origin         TEXT NOT NULL CHECK (origin IN ('extracted')),
  block_id       TEXT REFERENCES block(block_id),
  item_signature TEXT,
  user_modified  INTEGER NOT NULL DEFAULT 0,
  source_missing INTEGER NOT NULL DEFAULT 0,
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);
CREATE INDEX routine_block ON routine(block_id);

ALTER TABLE task ADD COLUMN routine_id TEXT REFERENCES routine(routine_id);
-- The routine slot (day) an occurrence fills; stays put when the user moves the task to another date.
ALTER TABLE task ADD COLUMN routine_date TEXT;
CREATE INDEX task_routine ON task(routine_id, routine_date);
