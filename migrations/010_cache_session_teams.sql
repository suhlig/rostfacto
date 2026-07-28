-- Existing sessions were created without cached team/admin data. Force a
-- fresh login so that every session is guaranteed to carry the new fields.
DELETE FROM sessions;

ALTER TABLE sessions
    ADD COLUMN is_admin BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN teams JSONB NOT NULL DEFAULT '[]'::jsonb;
