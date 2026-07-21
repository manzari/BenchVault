-- Reclassify plain-text files ingested before .txt document support existed.
-- They landed as generic attachments (kind 'other', role 'attachment'); move
-- them to the 'text' kind and 'document' role so they render verbatim in the
-- object's "Docs / Notes" section (e.g. LICENSE.txt / README.txt from imports).
-- The assets_au FTS trigger re-indexes on these UPDATEs, so no explicit FTS work.

UPDATE assets
SET kind = 'text', updated_at = strftime('%s','now')
WHERE kind = 'other' AND lower(original_filename) LIKE '%.txt';

UPDATE object_assets
SET role = 'document'
WHERE role = 'attachment'
  AND asset_id IN (SELECT id FROM assets WHERE kind = 'text');
