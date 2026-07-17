-- Generic per-mesh geometry sidecar, populated by STL now and OBJ/3MF/SCAD-render
-- later. Supersedes the never-populated `stl_metadata` table (v1 left it empty),
-- so there is no data to migrate.
DROP TABLE IF EXISTS stl_metadata;

CREATE TABLE mesh_metadata (
  asset_id INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
  source_format TEXT,           -- 'stl' | 'obj' | '3mf' | 'scad'
  source_units TEXT,            -- declared units when the format carries them (3mf); NULL for STL/OBJ
  triangle_count INTEGER,
  vertex_count INTEGER,         -- unique vertices after welding
  bbox_x_mm REAL,
  bbox_y_mm REAL,
  bbox_z_mm REAL,
  volume_mm3 REAL,
  surface_area_mm2 REAL,
  is_watertight INTEGER,        -- 0 | 1 | NULL (unknown)
  parsed_at INTEGER
);
