-- Mirror the UI tree on disk. Each folder/object owns a stable directory name,
-- assigned at create time and adjusted only when the entity is renamed/moved.
-- A NULL dir_name means "needs backfill" — the Rust startup pass fills these in
-- and relocates assets out of the old `assets/<yyyy>/<mm>/` layout.

ALTER TABLE folders ADD COLUMN dir_name TEXT;
ALTER TABLE objects ADD COLUMN dir_name TEXT;

CREATE INDEX idx_folders_dir_name ON folders(parent_id, dir_name);
CREATE INDEX idx_objects_dir_name ON objects(folder_id, dir_name);
