CREATE TYPE item_category AS ENUM ('GOOD', 'BAD', 'WATCH');

CREATE TABLE retrospectives (
    id SERIAL PRIMARY KEY,
    title TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE retro_items (
    id SERIAL PRIMARY KEY,
    retro_id INTEGER REFERENCES retrospectives(id),
    text TEXT NOT NULL,
    category item_category NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
