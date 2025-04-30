ALTER TABLE retrospectives ADD COLUMN slug TEXT NOT NULL;
ALTER TABLE retrospectives ADD CONSTRAINT retrospectives_slug_key UNIQUE (slug);
CREATE INDEX retrospectives_slug_idx ON retrospectives (slug);
