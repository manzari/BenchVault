-- Generic full-text search index across all primary entities.
-- The index stores an opaque (entity_kind, entity_id) and the searchable text columns.

CREATE VIRTUAL TABLE search_index USING fts5(
  entity_kind UNINDEXED,
  entity_id UNINDEXED,
  name,
  body,
  tokenize = 'porter unicode61'
);

-- Objects: name + notes
CREATE TRIGGER objects_ai AFTER INSERT ON objects
WHEN NEW.deleted_at IS NULL BEGIN
  INSERT INTO search_index(entity_kind, entity_id, name, body)
  VALUES ('object', NEW.id, NEW.name, COALESCE(NEW.notes, ''));
END;

CREATE TRIGGER objects_au AFTER UPDATE ON objects BEGIN
  DELETE FROM search_index WHERE entity_kind = 'object' AND entity_id = OLD.id;
  INSERT INTO search_index(entity_kind, entity_id, name, body)
  SELECT 'object', NEW.id, NEW.name, COALESCE(NEW.notes, '')
  WHERE NEW.deleted_at IS NULL;
END;

CREATE TRIGGER objects_ad AFTER DELETE ON objects BEGIN
  DELETE FROM search_index WHERE entity_kind = 'object' AND entity_id = OLD.id;
END;

-- Folders: name only (notes-less in v1)
CREATE TRIGGER folders_ai AFTER INSERT ON folders
WHEN NEW.deleted_at IS NULL BEGIN
  INSERT INTO search_index(entity_kind, entity_id, name, body)
  VALUES ('folder', NEW.id, NEW.name, '');
END;

CREATE TRIGGER folders_au AFTER UPDATE ON folders BEGIN
  DELETE FROM search_index WHERE entity_kind = 'folder' AND entity_id = OLD.id;
  INSERT INTO search_index(entity_kind, entity_id, name, body)
  SELECT 'folder', NEW.id, NEW.name, ''
  WHERE NEW.deleted_at IS NULL;
END;

CREATE TRIGGER folders_ad AFTER DELETE ON folders BEGIN
  DELETE FROM search_index WHERE entity_kind = 'folder' AND entity_id = OLD.id;
END;

-- Assets: original_filename + (eventually) tag-bound text
CREATE TRIGGER assets_ai AFTER INSERT ON assets
WHEN NEW.deleted_at IS NULL BEGIN
  INSERT INTO search_index(entity_kind, entity_id, name, body)
  VALUES ('asset', NEW.id, NEW.original_filename, '');
END;

CREATE TRIGGER assets_au AFTER UPDATE ON assets BEGIN
  DELETE FROM search_index WHERE entity_kind = 'asset' AND entity_id = OLD.id;
  INSERT INTO search_index(entity_kind, entity_id, name, body)
  SELECT 'asset', NEW.id, NEW.original_filename, ''
  WHERE NEW.deleted_at IS NULL;
END;

CREATE TRIGGER assets_ad AFTER DELETE ON assets BEGIN
  DELETE FROM search_index WHERE entity_kind = 'asset' AND entity_id = OLD.id;
END;
