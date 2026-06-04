use std::path::{Component, Path};

use sqlx::SqlitePool;

use crate::domain::{FolderId, ObjectId};
use crate::library::Library;
use crate::repository;

use super::ServiceResult;

/// Marker suffix that signals "this directory is an object" when the watcher needs to
/// auto-materialize tree nodes for a path it's never seen before.
pub const OBJECT_DIR_MARKER: &str = ".obj";

/// How `resolve_for_ingest` should treat a path whose owning object isn't in the DB yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestMode {
    /// Only resolve to objects that already exist; never create anything. Used when
    /// we just need to map a path to its (existing) owner.
    AttachOnly,
    /// Auto-create missing folders/objects, but only when the directory directly
    /// containing the file is tagged with the `.obj` marker. Conservative default
    /// for *live* watcher events, where transient activity from other tools should
    /// not spawn objects.
    AutoCreateMarked,
    /// Auto-create missing folders/objects for any asset-bearing directory, no marker
    /// required. Mirrors the on-disk tree into the DB. Used by the explicit / startup
    /// reconcile sweep, where the user has opted into "make the library match disk".
    AutoCreateMirror,
}

impl IngestMode {
    fn creates(self) -> bool {
        !matches!(self, IngestMode::AttachOnly)
    }
    fn requires_marker(self) -> bool {
        matches!(self, IngestMode::AutoCreateMarked)
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub object_id: ObjectId,
    pub file_name: String,
    pub rel_path: String,
}

/// Resolve `abs` (which must live under `library.root()`) to an object that should own
/// the file. `mode` controls whether missing folders/objects are created and whether the
/// `.obj` marker is required to do so (see [`IngestMode`]). The directory directly
/// containing the file is treated as the object; its ancestors are folders — matching the
/// canonical `<folder chain>/<object dir>/<files>` layout. Returns `Ok(None)` for paths we
/// deliberately ignore (loose root file, file under an unknown dir we won't create, file
/// outside the library, …). Folder creation is gated on the object segment being
/// resolvable, so we never leave orphan folder rows behind.
pub async fn resolve_for_ingest(
    pool: &SqlitePool,
    library: &Library,
    abs: &Path,
    mode: IngestMode,
) -> ServiceResult<Option<ResolvedTarget>> {
    let rel = match abs.strip_prefix(library.root()) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };

    let mut comps: Vec<String> = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(s) => match s.to_str() {
                Some(s) => comps.push(s.to_string()),
                None => return Ok(None),
            },
            Component::CurDir => {}
            _ => return Ok(None),
        }
    }
    if comps.is_empty() {
        return Ok(None);
    }

    let file_name = comps.pop().unwrap();
    if comps.is_empty() {
        eprintln!("[watcher] ignoring loose file at library root: {}", rel.display());
        return Ok(None);
    }
    let object_dir = comps.pop().unwrap();
    let folder_chain = comps;

    // Walk the folder chain non-mutatively. Once we hit a missing segment, every
    // remaining segment is queued for creation (because nested DB rows can't exist
    // under a non-existent parent).
    let mut current_folder_id: Option<FolderId> = None;
    let mut pending_folders: Vec<String> = Vec::new();
    for seg in &folder_chain {
        if !pending_folders.is_empty() {
            pending_folders.push(seg.clone());
            continue;
        }
        match repository::folders::find_by_parent_and_dir(pool, current_folder_id, seg).await? {
            Some(f) => current_folder_id = Some(f.id),
            None => pending_folders.push(seg.clone()),
        }
    }

    // The .obj marker is only valid on the *object* segment, never on a folder segment.
    if pending_folders.iter().any(|s| s.ends_with(OBJECT_DIR_MARKER)) {
        eprintln!(
            "[watcher] ignoring path: .obj marker on a non-leaf segment: {}",
            rel.display()
        );
        return Ok(None);
    }

    // If folders are pending, the object also can't exist yet. Otherwise look it up.
    let object_id = if pending_folders.is_empty() {
        match repository::objects::find_by_folder_and_dir(pool, current_folder_id, &object_dir)
            .await?
        {
            Some(o) => o.id,
            None => {
                if !may_create_object(mode, &object_dir) {
                    eprintln!(
                        "[watcher] ignoring file under unknown dir: {}",
                        rel.display()
                    );
                    return Ok(None);
                }
                create_object(pool, current_folder_id, &object_dir).await?
            }
        }
    } else {
        if !may_create_object(mode, &object_dir) {
            eprintln!(
                "[watcher] ignoring path needing folder creation: {}",
                rel.display()
            );
            return Ok(None);
        }
        // Commit the deferred folder creations now that we know the object will be created too.
        let mut parent = current_folder_id;
        for seg in &pending_folders {
            let created = repository::folders::create(pool, parent, seg, seg).await?;
            eprintln!(
                "[watcher] auto-created folder id={} dir_name='{}'",
                created.id.0, seg
            );
            parent = Some(created.id);
        }
        create_object(pool, parent, &object_dir).await?
    };

    let rel_str = rel.to_string_lossy().into_owned();
    Ok(Some(ResolvedTarget {
        object_id,
        file_name,
        rel_path: rel_str,
    }))
}

/// Whether `mode` permits materializing an object for `object_dir` that isn't in the DB:
/// the mode must create at all, and if it requires the marker the dir must carry it.
fn may_create_object(mode: IngestMode, object_dir: &str) -> bool {
    mode.creates() && (!mode.requires_marker() || object_dir.ends_with(OBJECT_DIR_MARKER))
}

async fn create_object(
    pool: &SqlitePool,
    folder_id: Option<FolderId>,
    object_dir: &str,
) -> ServiceResult<ObjectId> {
    let display_name = strip_marker(object_dir);
    let created = repository::objects::create(pool, folder_id, display_name, object_dir).await?;
    eprintln!(
        "[watcher] auto-created object id={} dir_name='{}' name='{}'",
        created.id.0, object_dir, display_name
    );
    Ok(created.id)
}

fn strip_marker(dir_name: &str) -> &str {
    dir_name
        .strip_suffix(OBJECT_DIR_MARKER)
        .map(|s| s.trim_end_matches('.').trim_end())
        .filter(|s| !s.is_empty())
        .unwrap_or(dir_name)
}
