-- Reclassify existing .scad assets from cad/attachment to scad/scad.
-- The assets_au FTS trigger re-indexes on this UPDATE, so no FTS work is needed.
UPDATE assets
SET kind = 'scad', updated_at = strftime('%s','now')
WHERE kind = 'cad' AND lower(original_filename) LIKE '%.scad';

UPDATE object_assets
SET role = 'scad'
WHERE role = 'attachment'
  AND asset_id IN (SELECT id FROM assets WHERE kind = 'scad');
