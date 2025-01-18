-- Drop the existing foreign key constraint
ALTER TABLE items
DROP CONSTRAINT items_retro_id_fkey;

-- Add it back with CASCADE
ALTER TABLE items
ADD CONSTRAINT items_retro_id_fkey
FOREIGN KEY (retro_id)
REFERENCES retrospectives(id)
ON DELETE CASCADE;
