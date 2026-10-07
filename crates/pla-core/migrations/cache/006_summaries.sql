-- FR-MEM-006/008: what a day's summary was made from (to know when it is stale), what embedded it,
-- and its keyword and vector indexes. A summary is only a way to find a day; raw notes answer.
ALTER TABLE daily_summary ADD COLUMN signature TEXT NOT NULL DEFAULT '';
ALTER TABLE daily_summary ADD COLUMN embedded_model TEXT;
CREATE VIRTUAL TABLE summary_fts USING fts5(date UNINDEXED, text);
CREATE VIRTUAL TABLE summary_vec USING vec0(day INTEGER PRIMARY KEY, embedding float[768] distance_metric=cosine);
-- a day's notes are looked up by modification time (final review I1)
CREATE INDEX note_index_mtime ON note_index(mtime);
