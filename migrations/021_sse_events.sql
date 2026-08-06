-- Durable event log that powers cross-client sync via SSE.
--
-- Every interesting mutation on items, likes, or archives writes a row here
-- (via triggers below), and then NOTIFYs the 'rostfacto_events' channel so
-- each app instance's notifier task can fan the event out to connected
-- browsers.  The table doubles as the replay source: a reconnecting client
-- sends Last-Event-ID and the server replays events with a larger id.

CREATE TYPE event_type AS ENUM (
    'ITEM_CREATED', 'ITEM_UPDATED', 'ITEM_STATUS_CHANGED',
    'ITEM_LIKED', 'ITEM_UNLIKED',
    'TIMER_STARTED', 'TIMER_EXTENDED', 'TIMER_CANCELLED', 'TIMER_ELAPSED',
    'RETRO_ARCHIVED'
);

CREATE TABLE events (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    retro_id   INTEGER NOT NULL REFERENCES retrospectives(id) ON DELETE CASCADE,
    event_type event_type NOT NULL,
    item_id    INTEGER,          -- NULL for retro-level events
    payload    JSONB NOT NULL,   -- structured event data
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX events_retro_id_id_idx ON events(retro_id, id);

-- Emits one event row (plus a NOTIFY) for item inserts and for updates that
-- change the text or the status.  Archive updates are excluded by the
-- trigger's WHEN clause (see below) so bulk archiving emits a single
-- RETRO_ARCHIVED event instead of per-item noise.
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

CREATE TRIGGER items_insert_event
    AFTER INSERT ON items
    FOR EACH ROW
    EXECUTE FUNCTION emit_item_event();

-- Skip bulk archive updates: they change status AND archive_id together, and
-- the single RETRO_ARCHIVED event (from the archives INSERT trigger below) is
-- the only event an archive operation should produce.
CREATE TRIGGER items_update_event
    AFTER UPDATE ON items
    FOR EACH ROW
    WHEN (OLD.archive_id IS NOT DISTINCT FROM NEW.archive_id)
    EXECUTE FUNCTION emit_item_event();

-- Emits ITEM_LIKED / ITEM_UNLIKED with the recomputed likes count.  The
-- retro_id is looked up from the item, and events are skipped when the parent
-- item is already gone (cascade delete of the retro), so a retro deletion
-- cannot fail or spam the log.
CREATE OR REPLACE FUNCTION emit_like_event()
RETURNS TRIGGER AS $$
DECLARE
    v_item_id  INTEGER;
    v_retro_id INTEGER;
    v_count    BIGINT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        v_item_id := OLD.item_id;
    ELSE
        v_item_id := NEW.item_id;
    END IF;

    SELECT retro_id INTO v_retro_id FROM items WHERE id = v_item_id;
    IF v_retro_id IS NULL THEN
        RETURN NULL; -- parent retro/item is being cascade-deleted
    END IF;

    SELECT COUNT(*) INTO v_count FROM likes WHERE item_id = v_item_id;

    INSERT INTO events (retro_id, event_type, item_id, payload)
    VALUES (
        v_retro_id,
        CASE WHEN TG_OP = 'INSERT' THEN 'ITEM_LIKED'::event_type
             ELSE 'ITEM_UNLIKED'::event_type END,
        v_item_id,
        jsonb_build_object('item_id', v_item_id, 'likes_count', v_count)
    );

    PERFORM pg_notify('rostfacto_events', v_retro_id::text);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER likes_insert_event
    AFTER INSERT ON likes
    FOR EACH ROW
    EXECUTE FUNCTION emit_like_event();

CREATE TRIGGER likes_delete_event
    AFTER DELETE ON likes
    FOR EACH ROW
    EXECUTE FUNCTION emit_like_event();

-- Fires exactly once per archive operation: the handler only inserts an
-- archives row when there is something to archive.
CREATE OR REPLACE FUNCTION emit_archive_event()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO events (retro_id, event_type, payload)
    VALUES (NEW.retro_id, 'RETRO_ARCHIVED', jsonb_build_object('retro_id', NEW.retro_id));

    PERFORM pg_notify('rostfacto_events', NEW.retro_id::text);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER archives_insert_event
    AFTER INSERT ON archives
    FOR EACH ROW
    EXECUTE FUNCTION emit_archive_event();
