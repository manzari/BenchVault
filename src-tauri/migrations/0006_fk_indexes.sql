-- Index the child side of two foreign keys that were previously unindexed.
-- Without these, deleting a tag scans all of entity_tags to cascade, and every
-- detach_asset / asset hard-delete (which filter or cascade by asset_id) scans
-- the whole object_assets table.

CREATE INDEX IF NOT EXISTS idx_entity_tags_tag ON entity_tags(tag_id);
CREATE INDEX IF NOT EXISTS idx_object_assets_asset ON object_assets(asset_id);
