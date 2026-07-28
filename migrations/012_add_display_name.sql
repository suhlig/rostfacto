ALTER TABLE users
    ADD COLUMN display_name TEXT
    GENERATED ALWAYS AS (COALESCE(full_name, username)) VIRTUAL;
