-- Action items now participate in cross-client sync: their mutations write
-- `events` rows (via the `emit_event` helper in src/handlers.rs) like every
-- other board mutation. Extend the event enum with the action-item event types.
--
-- Adding enum values inside the migration transaction is safe on PostgreSQL 12+
-- as long as the new values are not used until the transaction commits.
ALTER TYPE event_type ADD VALUE IF NOT EXISTS 'ACTION_ITEM_CREATED';
ALTER TYPE event_type ADD VALUE IF NOT EXISTS 'ACTION_ITEM_UPDATED';
ALTER TYPE event_type ADD VALUE IF NOT EXISTS 'ACTION_ITEM_COMPLETED';
ALTER TYPE event_type ADD VALUE IF NOT EXISTS 'ACTION_ITEM_DELETED';
