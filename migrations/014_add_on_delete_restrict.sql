ALTER TABLE items
    DROP CONSTRAINT items_created_by_fkey,
    ADD CONSTRAINT items_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE RESTRICT;

ALTER TABLE retrospectives
    DROP CONSTRAINT retrospectives_created_by_fkey,
    ADD CONSTRAINT retrospectives_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE RESTRICT;
