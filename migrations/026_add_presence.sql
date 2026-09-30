-- Ephemeral presence coordination state for the retro participant panel.
--
-- Multi-process by design: several app processes share this one database, so
-- presence lives here (not in memory) and every process sees the same roster.
-- Nothing in this table is durable history -- it is safe to truncate at any
-- time, and rows age out via `last_seen_at` freshness (heartbeat) rather than
-- an explicit leave.
--
-- One row per (retro, participant, instance): a participant connected to two
-- processes has two rows; the roster collapses them with
-- `DISTINCT ON (participant_key)` taking the earliest `joined_at`.

-- Global guest numbering: unique across processes but not contiguous per
-- retro (an accepted trade-off for cross-process uniqueness without a hot
-- per-retro counter row).
CREATE SEQUENCE presence_guest_number_seq;

CREATE TABLE presence (
    retro_id        INTEGER NOT NULL REFERENCES retrospectives(id) ON DELETE CASCADE,
    -- 'user:{users.id}' in auth mode, 'guest:{uuid}' in demo mode.
    participant_key TEXT NOT NULL,
    -- Per-process id, regenerated on restart. Scopes heartbeats and lets
    -- graceful shutdown delete exactly this process's rows.
    instance_id     UUID NOT NULL,
    -- users.display_name, or 'Guest N' from the sequence.
    name            TEXT NOT NULL,
    -- NULL in demo mode -> initials circle.
    avatar_url      TEXT,
    -- Precomputed fallback ('JD', 'G1').
    initials        TEXT NOT NULL,
    guest           BOOLEAN NOT NULL,
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (retro_id, participant_key, instance_id)
);

CREATE INDEX presence_retro_fresh_idx ON presence (retro_id, last_seen_at);
CREATE INDEX presence_instance_idx ON presence (instance_id);
