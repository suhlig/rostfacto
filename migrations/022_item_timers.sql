-- Server-authoritative highlight timers.
--
-- Timer state lives on the item itself; the deadline is a PG 18 virtual
-- generated column so the sweep query and every client derive the same value
-- from the same stored state.

ALTER TABLE items
    ADD COLUMN timer_started_at TIMESTAMPTZ,
    ADD COLUMN timer_duration_seconds INTEGER,
    -- PG 18 virtual generated column: all clients and the sweep query derive
    -- the same deadline from the same stored state.  (Epoch arithmetic is
    -- used because timestamptz + interval and EXTRACT(EPOCH FROM timestamptz)
    -- are only stable, not immutable; subtracting the epoch constant first
    -- routes EXTRACT to the immutable interval overload.)
    ADD COLUMN timer_ends_at TIMESTAMPTZ GENERATED ALWAYS AS
        (to_timestamp(EXTRACT(EPOCH FROM (timer_started_at - '1970-01-01 00:00:00+00'::timestamptz)) + timer_duration_seconds)) VIRTUAL,
    ADD COLUMN timer_elapsed_at TIMESTAMPTZ;

-- Extend the item event trigger with timer events.  Precedence when one
-- UPDATE changes several things at once:
--   1. ITEM_CREATED (insert)
--   2. ITEM_STATUS_CHANGED wins over timer changes: highlighting starts the
--      timer implicitly, completing and cancelling reset the timer columns in
--      the same UPDATE as the status change.
--   3. TIMER_ELAPSED when the sweep marks a timer elapsed.
--   4. TIMER_STARTED when a timer begins (fresh start; a restart after
--      elapsed also lands here because the elapsed marker is cleared).
--   5. TIMER_EXTENDED for any other timer column change (extend, and a start
--      POST on an already running timer).
--   6. ITEM_UPDATED for text-only changes.
-- TIMER_CANCELLED is never emitted: cancellation is always observed as the
-- status transition back to CREATED.
CREATE OR REPLACE FUNCTION emit_item_event()
RETURNS TRIGGER AS $$
DECLARE
    v_event_type event_type;
    v_payload    JSONB;
BEGIN
    IF TG_OP = 'INSERT' THEN
        v_event_type := 'ITEM_CREATED';
        v_payload := jsonb_build_object(
            'item_id', NEW.id,
            'retro_id', NEW.retro_id,
            'category', NEW.category,
            'text', NEW.text,
            'status', NEW.status,
            'likes_count', 0,
            -- The client re-fetches /items/{id} for full card re-renders, so
            -- only the author name is included here; author initials are
            -- derived per-retro by the app (disambiguation), not in SQL.
            'author_name', (SELECT display_name FROM users WHERE id = NEW.created_by)
        );
    ELSIF OLD.status IS DISTINCT FROM NEW.status THEN
        v_event_type := 'ITEM_STATUS_CHANGED';
        v_payload := jsonb_build_object(
            'item_id', NEW.id,
            'old_status', OLD.status,
            'new_status', NEW.status
        );
    ELSIF OLD.timer_elapsed_at IS NULL AND NEW.timer_elapsed_at IS NOT NULL THEN
        v_event_type := 'TIMER_ELAPSED';
        v_payload := jsonb_build_object('item_id', NEW.id);
    ELSIF OLD.timer_started_at IS NULL AND NEW.timer_started_at IS NOT NULL THEN
        v_event_type := 'TIMER_STARTED';
        v_payload := jsonb_build_object(
            'item_id', NEW.id,
            'duration_seconds', NEW.timer_duration_seconds,
            'started_at', NEW.timer_started_at,
            -- The timer_ends_at virtual generated column reads as NULL from
            -- trigger NEW, so compute the deadline here instead.
            'ends_at', NEW.timer_started_at + (NEW.timer_duration_seconds * INTERVAL '1 second')
        );
    ELSIF OLD.timer_started_at IS DISTINCT FROM NEW.timer_started_at
       OR OLD.timer_duration_seconds IS DISTINCT FROM NEW.timer_duration_seconds
       OR OLD.timer_elapsed_at IS DISTINCT FROM NEW.timer_elapsed_at THEN
        v_event_type := 'TIMER_EXTENDED';
        v_payload := jsonb_build_object(
            'item_id', NEW.id,
            'duration_seconds', NEW.timer_duration_seconds,
            'started_at', NEW.timer_started_at,
            'ends_at', NEW.timer_started_at + (NEW.timer_duration_seconds * INTERVAL '1 second')
        );
    ELSIF OLD.text IS DISTINCT FROM NEW.text THEN
        v_event_type := 'ITEM_UPDATED';
        v_payload := jsonb_build_object('item_id', NEW.id, 'text', NEW.text);
    ELSE
        RETURN NULL; -- no interesting change (e.g. only updated_at)
    END IF;

    INSERT INTO events (retro_id, event_type, item_id, payload)
    VALUES (NEW.retro_id, v_event_type, NEW.id, v_payload);

    PERFORM pg_notify('rostfacto_events', NEW.retro_id::text);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
