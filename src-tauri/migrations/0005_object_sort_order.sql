-- User-controlled ordering for objects within their parent folder.
-- The tree sorts ascending by sort_order, with id as a stable tiebreaker.
-- Initialize from id so existing objects keep their insertion order.

ALTER TABLE objects
  ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

UPDATE objects SET sort_order = id;

CREATE INDEX idx_objects_folder_sort ON objects(folder_id, sort_order);
