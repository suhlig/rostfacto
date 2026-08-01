CREATE TABLE archives (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    retro_id INTEGER NOT NULL REFERENCES retrospectives(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX archives_retro_id_idx ON archives(retro_id);

ALTER TABLE items
ADD COLUMN archive_id INTEGER REFERENCES archives(id) ON DELETE SET NULL,
ADD COLUMN archived_at TIMESTAMPTZ;

CREATE INDEX items_archive_id_idx ON items(archive_id);

-- Backfill existing archived items: create one archive per retro that has them
INSERT INTO archives (retro_id, created_at)
SELECT DISTINCT retro_id, NOW()
FROM items
WHERE status = 'ARCHIVED';

UPDATE items
SET archive_id = archives.id,
    archived_at = NOW()
FROM archives
WHERE items.status = 'ARCHIVED'
  AND items.retro_id = archives.retro_id;
