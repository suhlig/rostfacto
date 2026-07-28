ALTER TABLE sessions
    ALTER COLUMN id SET DEFAULT uuidv7()::text;
