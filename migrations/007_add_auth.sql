CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    github_id BIGINT NOT NULL UNIQUE,
    username TEXT NOT NULL,
    avatar_url TEXT,
    access_token TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX sessions_user_id_idx ON sessions(user_id);
CREATE INDEX sessions_expires_at_idx ON sessions(expires_at);

ALTER TABLE retrospectives ADD COLUMN team_slug TEXT NOT NULL;
ALTER TABLE retrospectives ADD COLUMN created_by INTEGER NOT NULL REFERENCES users(id);
