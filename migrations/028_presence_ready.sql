-- Whether a participant has indicated they are done writing cards.
--
-- Part of the ephemeral presence state (see 026_add_presence.sql): it is
-- per-participant coordination state, not durable history, and it disappears
-- with the row when the participant ages out. `PresenceHub::set_ready` updates
-- every row of a participant (they may be connected to several processes at
-- once); a rejoin carries the state over from a still-fresh row.
ALTER TABLE presence ADD COLUMN ready BOOLEAN NOT NULL DEFAULT FALSE;
