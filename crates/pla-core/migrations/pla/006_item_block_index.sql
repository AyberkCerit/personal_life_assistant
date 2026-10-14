-- Architecture review A10: task had an index on block_id, metric_record and review_item did not.
-- Every block that goes missing updates metric_record by block_id, and extraction reads both by it.
CREATE INDEX metric_record_block ON metric_record (block_id);
CREATE INDEX review_item_block ON review_item (block_id);
