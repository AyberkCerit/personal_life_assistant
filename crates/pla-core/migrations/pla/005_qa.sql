-- FR-QA-014/015: the assistant's turns. Appendix C's table gains how a turn ended and where a new
-- topic starts (only the turns since then go back to the model). Never indexed (FR-MEM-014).
ALTER TABLE qa_turn ADD COLUMN status TEXT NOT NULL DEFAULT 'done' CHECK (status IN ('done', 'stopped', 'failed'));
ALTER TABLE qa_turn ADD COLUMN new_topic INTEGER NOT NULL DEFAULT 0;
CREATE INDEX qa_turn_created ON qa_turn (created_at);
