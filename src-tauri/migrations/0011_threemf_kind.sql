-- Reclassify .3mf files ingested before the 'threemf' kind existed. They landed
-- as 'stl' (classify_model_kind's fallback) or as generic attachments, so the
-- mesh backfill fed a ZIP container to the STL/OBJ parser -- "[mesh] parse
-- failed ... stream did not contain valid UTF-8". Moving them to 'threemf' routes
-- them through the 3MF backfill instead (geometry, units, thumbnail cover).
-- The assets_au FTS trigger re-indexes on these UPDATEs, so no explicit FTS work.

UPDATE assets
SET kind = 'threemf', mime_type = COALESCE(mime_type, 'model/3mf'), updated_at = strftime('%s','now')
WHERE kind <> 'threemf' AND lower(original_filename) LIKE '%.3mf';

UPDATE object_assets
SET role = 'stl_part'
WHERE role = 'attachment'
  AND asset_id IN (SELECT id FROM assets WHERE kind = 'threemf');
