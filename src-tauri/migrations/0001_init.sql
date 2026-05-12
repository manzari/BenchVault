-- Generic primitives (used by every present and future entity)

CREATE TABLE assets (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  relative_path TEXT NOT NULL,
  original_filename TEXT NOT NULL,
  mime_type TEXT,
  size_bytes INTEGER,
  sha256 TEXT,
  metadata_json TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);
CREATE INDEX idx_assets_kind ON assets(kind);
CREATE INDEX idx_assets_sha ON assets(sha256);

CREATE TABLE tags (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  color TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE entity_tags (
  entity_kind TEXT NOT NULL,
  entity_id INTEGER NOT NULL,
  tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (entity_kind, entity_id, tag_id)
);
CREATE INDEX idx_entity_tags_lookup ON entity_tags(entity_kind, entity_id);

CREATE TABLE entity_links (
  id INTEGER PRIMARY KEY,
  from_kind TEXT NOT NULL,
  from_id INTEGER NOT NULL,
  to_kind TEXT NOT NULL,
  to_id INTEGER NOT NULL,
  relation TEXT NOT NULL,
  metadata_json TEXT,
  created_at INTEGER NOT NULL,
  UNIQUE (from_kind, from_id, to_kind, to_id, relation)
);
CREATE INDEX idx_links_from ON entity_links(from_kind, from_id);
CREATE INDEX idx_links_to ON entity_links(to_kind, to_id);

-- v1 domain entities

CREATE TABLE folders (
  id INTEGER PRIMARY KEY,
  parent_id INTEGER REFERENCES folders(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  metadata_json TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);
CREATE INDEX idx_folders_parent ON folders(parent_id);

CREATE TABLE objects (
  id INTEGER PRIMARY KEY,
  folder_id INTEGER REFERENCES folders(id) ON DELETE SET NULL,
  name TEXT NOT NULL,
  notes TEXT,
  source_url TEXT,
  author TEXT,
  license TEXT,
  cover_asset_id INTEGER REFERENCES assets(id) ON DELETE SET NULL,
  metadata_json TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);
CREATE INDEX idx_objects_folder ON objects(folder_id);

CREATE TABLE object_assets (
  object_id INTEGER NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
  asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  role TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (object_id, asset_id, role)
);
CREATE INDEX idx_object_assets_object ON object_assets(object_id);

CREATE TABLE gcode_metadata (
  asset_id INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
  slicer_name TEXT,
  slicer_version TEXT,
  profile_name TEXT,
  layer_height_mm REAL,
  nozzle_temp_c INTEGER,
  bed_temp_c INTEGER,
  estimated_time_seconds INTEGER,
  filament_length_mm REAL,
  filament_weight_g REAL,
  filament_type TEXT,
  parsed_at INTEGER
);

CREATE TABLE stl_metadata (
  asset_id INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
  triangle_count INTEGER,
  bbox_x_mm REAL,
  bbox_y_mm REAL,
  bbox_z_mm REAL
);
