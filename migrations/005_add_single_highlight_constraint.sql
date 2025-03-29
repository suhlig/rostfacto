-- Add a partial unique index to enforce single highlighted item per retro
CREATE UNIQUE INDEX single_highlighted_item_per_retro
ON items (retro_id)
WHERE status = 'HIGHLIGHTED'::status;
