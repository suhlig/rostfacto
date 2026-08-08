-- Cap the size of user-supplied text. The app validates the same limits in
-- the handlers (MAX_ITEM_TEXT_LENGTH / MAX_RETRO_TITLE_LENGTH in
-- src/handlers.rs); these CHECKs are the last line of defense against
-- oversized rows that would bloat every board render and SSE payload.
ALTER TABLE retrospectives
    ADD CONSTRAINT retrospectives_title_length_check
    CHECK (length(title) <= 200);

ALTER TABLE items
    ADD CONSTRAINT items_text_length_check
    CHECK (length(text) <= 5000);

ALTER TABLE action_items
    ADD CONSTRAINT action_items_text_length_check
    CHECK (length(text) <= 5000);
