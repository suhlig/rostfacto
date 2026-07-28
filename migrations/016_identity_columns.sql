-- Convert remaining SERIAL columns to GENERATED ALWAYS AS IDENTITY.
-- Sessions.id is a text token, not an integer, so it is left unchanged.

ALTER TABLE retrospectives ALTER COLUMN id DROP DEFAULT;
DROP SEQUENCE retrospectives_id_seq;
ALTER TABLE retrospectives ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY;
SELECT setval(
    'retrospectives_id_seq',
    COALESCE((SELECT MAX(id) FROM retrospectives), 0) + 1,
    false
);

ALTER TABLE items ALTER COLUMN id DROP DEFAULT;
DROP SEQUENCE items_id_seq;
ALTER TABLE items ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY;
SELECT setval(
    'items_id_seq',
    COALESCE((SELECT MAX(id) FROM items), 0) + 1,
    false
);

ALTER TABLE users ALTER COLUMN id DROP DEFAULT;
DROP SEQUENCE users_id_seq;
ALTER TABLE users ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY;
SELECT setval(
    'users_id_seq',
    COALESCE((SELECT MAX(id) FROM users), 0) + 1,
    false
);
