-- `![[Note]]` embeds became links: every note is parsed again at the next sync (size -1, see
-- `index::refresh`). The rows stay as they are: a row missing, or a changed mtime or hash, would
-- tell the memory step a note is gone or changed and drop day summaries.
UPDATE note_index SET size = -1;
