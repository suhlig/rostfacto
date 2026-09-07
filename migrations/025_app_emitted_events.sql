-- Events are now emitted by the application itself: every mutation writes its
-- `events` row (and NOTIFYs the `rostfacto_events` channel) inside the same
-- transaction via the `emit_event` helper in src/handlers.rs. The DB trigger
-- machinery from migrations 021/022 is therefore removed.
--
-- The `events` table, the `event_type` enum, the channel name, and the
-- `updated_at` triggers (migration 015) are all unchanged.

DROP TRIGGER IF EXISTS items_insert_event ON items;
DROP TRIGGER IF EXISTS items_update_event ON items;
DROP TRIGGER IF EXISTS likes_insert_event ON likes;
DROP TRIGGER IF EXISTS likes_delete_event ON likes;
DROP TRIGGER IF EXISTS archives_insert_event ON archives;

DROP FUNCTION IF EXISTS emit_item_event();
DROP FUNCTION IF EXISTS emit_like_event();
DROP FUNCTION IF EXISTS emit_archive_event();
