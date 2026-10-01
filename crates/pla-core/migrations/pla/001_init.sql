CREATE TABLE block (
  block_id      TEXT PRIMARY KEY,
  note_path     TEXT NOT NULL,
  position      INTEGER NOT NULL,
  text_hash     TEXT NOT NULL,
  first_seen_at TEXT NOT NULL,
  last_seen_at  TEXT NOT NULL,
  missing       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX block_note ON block(note_path);

CREATE TABLE task (
  task_id        TEXT PRIMARY KEY,
  title          TEXT NOT NULL CHECK (length(title) <= 200),
  details        TEXT,
  date           TEXT,
  time           TEXT,
  notify_at      TEXT,
  status         TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'done', 'cancelled')),
  origin         TEXT NOT NULL CHECK (origin IN ('manual', 'extracted', 'assistant')),
  block_id       TEXT REFERENCES block(block_id),
  item_signature TEXT,
  user_modified  INTEGER NOT NULL DEFAULT 0,
  source_missing INTEGER NOT NULL DEFAULT 0,
  completed_at   TEXT,
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);
CREATE INDEX task_date ON task(date);
CREATE INDEX task_block ON task(block_id);

CREATE TABLE metric_record (
  metric_id      TEXT PRIMARY KEY,
  type           TEXT NOT NULL CHECK (type IN ('sleep', 'weight', 'steps', 'water', 'workout')),
  value_json     TEXT NOT NULL,
  unit           TEXT,
  date           TEXT NOT NULL,
  origin         TEXT NOT NULL CHECK (origin IN ('manual', 'extracted', 'assistant')),
  block_id       TEXT REFERENCES block(block_id),
  item_signature TEXT,
  user_modified  INTEGER NOT NULL DEFAULT 0,
  source_missing INTEGER NOT NULL DEFAULT 0,
  created_at     TEXT NOT NULL
);
CREATE INDEX metric_type_date ON metric_record(type, date);

CREATE TABLE rejection (
  rejection_id   TEXT PRIMARY KEY,
  block_id       TEXT NOT NULL REFERENCES block(block_id),
  item_signature TEXT NOT NULL,
  rejected_at    TEXT NOT NULL
);
CREATE UNIQUE INDEX rejection_block_signature ON rejection(block_id, item_signature);

CREATE TABLE review_item (
  review_id    TEXT PRIMARY KEY,
  block_id     TEXT REFERENCES block(block_id),
  payload_json TEXT NOT NULL,
  reason       TEXT NOT NULL,
  created_at   TEXT NOT NULL,
  resolved     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE job_run (
  job_type        TEXT PRIMARY KEY CHECK (job_type IN ('daily_maintenance', 'weekly_report', 'backup')),
  last_success_at TEXT,
  last_attempt_at TEXT,
  last_error      TEXT,
  attempts        INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE qa_turn (
  turn_id         TEXT PRIMARY KEY,
  question        TEXT NOT NULL,
  answer          TEXT NOT NULL,
  tool_calls_json TEXT,
  created_at      TEXT NOT NULL
);
