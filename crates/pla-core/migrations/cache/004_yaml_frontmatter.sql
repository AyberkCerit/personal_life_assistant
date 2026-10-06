-- media final review I4: frontmatter tags and aliases come from the YAML parser the properties
-- strip uses; the cache is rebuilt so both agree.
DELETE FROM note_index;
DELETE FROM note_fts;
DELETE FROM link;
