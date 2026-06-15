-- USB sync feature.
-- 1) Per-gcode "export to USB" toggle, default ON. Lives on the kind-specific
--    sidecar so the polymorphic `assets` table stays neutral.
-- 2) Generic key/value bag for app preferences (USB target, sync manifest,
--    last-sync timestamps, last error). One row per key, JSON in value.

ALTER TABLE gcode_metadata
  ADD COLUMN export_to_usb INTEGER NOT NULL DEFAULT 1;

CREATE TABLE app_settings (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
