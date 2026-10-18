-- Routines (spec 2026-10-18): before them a weekly habit became one task per day, all with the same
-- signature in one block. Such blocks are extracted once more so the habit becomes a routine; this only
-- re-queues, and the routine's takeover decides which of those tasks go (done or edited ones stay).
CREATE TEMP TABLE repeated_block AS
  SELECT DISTINCT block_id FROM task
  WHERE origin = 'extracted' AND routine_id IS NULL AND block_id IS NOT NULL AND item_signature IS NOT NULL
  GROUP BY block_id, item_signature HAVING count(*) >= 2;

UPDATE block SET extracted_hash = NULL WHERE block_id IN (SELECT block_id FROM repeated_block);

INSERT OR IGNORE INTO extraction_queue (note_path, queued_at)
  SELECT DISTINCT note_path, strftime('%Y-%m-%dT%H:%M:%SZ', 'now') FROM block WHERE missing = 0 AND block_id IN (SELECT block_id FROM repeated_block);

DROP TABLE repeated_block;

-- A one-off copy a routine took over is cancelled with the routine's id, so undoing the routine reopens it.
ALTER TABLE task ADD COLUMN superseded_by TEXT REFERENCES routine(routine_id);
