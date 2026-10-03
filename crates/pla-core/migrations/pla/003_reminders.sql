-- When the reminder's notification was shown (local YYYY-MM-DDTHH:MM); NULL = still to show.
ALTER TABLE task ADD COLUMN notified_at TEXT;
