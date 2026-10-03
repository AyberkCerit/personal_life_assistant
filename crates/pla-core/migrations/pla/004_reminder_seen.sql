-- When the user closed a shown reminder in the banner without answering it (local YYYY-MM-DDTHH:MM);
-- cleared each time the reminder is shown again. Shown, open and not closed = still listed.
ALTER TABLE task ADD COLUMN reminder_seen_at TEXT;
