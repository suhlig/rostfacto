CREATE TABLE action_items (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    retro_id INTEGER NOT NULL REFERENCES retrospectives(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    archive_id INTEGER REFERENCES archives(id) ON DELETE SET NULL,
    archived_at TIMESTAMPTZ,
    CONSTRAINT action_items_text_not_blank CHECK (length(btrim(text)) > 0)
);

CREATE INDEX action_items_retro_id_idx ON action_items(retro_id);
CREATE INDEX action_items_archive_id_idx ON action_items(archive_id);
