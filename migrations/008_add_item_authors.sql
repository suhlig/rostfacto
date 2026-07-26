ALTER TABLE users ADD COLUMN full_name TEXT;

ALTER TABLE items ADD COLUMN created_by INTEGER REFERENCES users(id);

UPDATE items
SET created_by = retrospectives.created_by
FROM retrospectives
WHERE items.retro_id = retrospectives.id;

ALTER TABLE items ALTER COLUMN created_by SET NOT NULL;
CREATE INDEX items_created_by_idx ON items(created_by);
