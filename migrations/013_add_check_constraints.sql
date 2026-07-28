ALTER TABLE retrospectives
    ADD CONSTRAINT retrospectives_slug_format_check
    CHECK (slug ~ '^[a-z0-9-]+$' AND length(slug) <= 255);

ALTER TABLE items
    ADD CONSTRAINT items_text_not_empty_check
    CHECK (length(trim(text)) > 0);

ALTER TABLE users
    ADD CONSTRAINT users_username_not_empty_check
    CHECK (length(trim(username)) > 0);
