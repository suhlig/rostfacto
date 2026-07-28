CREATE INDEX items_retro_category_status_idx
ON items (retro_id, category, status)
INCLUDE (text, created_at, created_by);
