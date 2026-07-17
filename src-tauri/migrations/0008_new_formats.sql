-- Reclassify pre-existing assets to the per-format kinds introduced alongside
-- OBJ preview, STEP, CadQuery scripts and Markdown docs. The assets_au FTS
-- trigger re-indexes on these UPDATEs, so no explicit FTS work is needed.
--
-- Note: CadQuery `.py` scripts are identified by file content (a `cadquery`
-- import), which SQL can't sniff. Existing `.py` files stay as generic
-- attachments until re-added or picked up by a rescan.

-- .obj: dedicated kind, still a model in the "Models" section.
UPDATE assets
SET kind = 'obj', updated_at = strftime('%s','now')
WHERE kind IN ('stl', 'cad') AND lower(original_filename) LIKE '%.obj';

-- .step / .stp: dedicated kind, promoted from attachment to a model part.
UPDATE assets
SET kind = 'step', updated_at = strftime('%s','now')
WHERE kind = 'cad'
  AND (lower(original_filename) LIKE '%.step' OR lower(original_filename) LIKE '%.stp');

UPDATE object_assets
SET role = 'stl_part'
WHERE role = 'attachment'
  AND asset_id IN (SELECT id FROM assets WHERE kind IN ('obj', 'step'));

-- .md / .markdown: dedicated kind rendered in the "Docs / Notes" section.
UPDATE assets
SET kind = 'markdown', updated_at = strftime('%s','now')
WHERE kind = 'other'
  AND (lower(original_filename) LIKE '%.md' OR lower(original_filename) LIKE '%.markdown');

UPDATE object_assets
SET role = 'document'
WHERE role = 'attachment'
  AND asset_id IN (SELECT id FROM assets WHERE kind = 'markdown');
