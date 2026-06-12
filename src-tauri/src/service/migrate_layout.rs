//! One-shot migration that brings on-disk state in line with the folder/object tree.
//!
//! Runs every startup but is idempotent: it only does work for folders/objects whose
//! `dir_name` is still NULL, and for assets whose `relative_path` still starts with the
//! pre-mirror `assets/` prefix.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use sqlx::{Row, SqlitePool};

use crate::library::{self, Library};
use crate::repository;

use super::folders::relative_dir;
use super::ServiceResult;

pub async fn run(pool: &SqlitePool, library: &Library) -> ServiceResult<()> {
    backfill_folder_dir_names(pool).await?;
    backfill_object_dir_names(pool).await?;
    relocate_legacy_assets(pool, library).await?;
    cleanup_legacy_assets_root(library);
    Ok(())
}

async fn backfill_folder_dir_names(pool: &SqlitePool) -> ServiceResult<()> {
    let rows = sqlx::query(
        "SELECT id, parent_id, name FROM folders WHERE dir_name IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    for r in rows {
        let id = crate::domain::FolderId(r.get::<i64, _>("id"));
        let parent_id = r
            .try_get::<Option<i64>, _>("parent_id")
            .unwrap_or(None)
            .map(crate::domain::FolderId);
        let name: String = r.get("name");
        let taken =
            repository::folders::sibling_dir_names_with_objects(pool, parent_id, Some(id), None)
                .await?;
        let dir_name = library::pick_dir_name(&name, &taken, id.0);
        repository::folders::set_dir_name(pool, id, &dir_name).await?;
    }
    Ok(())
}

async fn backfill_object_dir_names(pool: &SqlitePool) -> ServiceResult<()> {
    let rows = sqlx::query(
        "SELECT id, folder_id, name FROM objects WHERE dir_name IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    for r in rows {
        let id = crate::domain::ObjectId(r.get::<i64, _>("id"));
        let folder_id = r
            .try_get::<Option<i64>, _>("folder_id")
            .unwrap_or(None)
            .map(crate::domain::FolderId);
        let name: String = r.get("name");
        let taken = repository::folders::sibling_dir_names_with_objects(
            pool,
            folder_id,
            None,
            Some(id.0),
        )
        .await?;
        let dir_name = library::pick_dir_name(&name, &taken, id.0);
        repository::objects::set_dir_name(pool, id, &dir_name).await?;
    }
    Ok(())
}

/// Move any asset whose path lives under the old `assets/<yyyy>/<mm>/...` layout to
/// `<folder chain>/<object dir>/<filename>`. Orphan assets (no object) get parked under
/// `_orphans/`.
async fn relocate_legacy_assets(pool: &SqlitePool, library: &Library) -> ServiceResult<()> {
    let rows = sqlx::query(
        "SELECT a.id, a.relative_path, a.original_filename, oa.object_id
         FROM assets a
         LEFT JOIN object_assets oa ON oa.asset_id = a.id
         WHERE a.relative_path LIKE 'assets/%'",
    )
    .fetch_all(pool)
    .await?;

    for r in rows {
        let asset_id = crate::domain::AssetId(r.get::<i64, _>("id"));
        let old_rel: String = r.get("relative_path");
        let original: String = r.get("original_filename");
        let object_id = r.try_get::<Option<i64>, _>("object_id").unwrap_or(None);

        let target_dir_rel = match object_id {
            Some(oid) => target_dir_for_object(pool, library, oid).await?,
            None => PathBuf::from("_orphans"),
        };

        let target_dir_abs = library.absolute(&target_dir_rel);
        std::fs::create_dir_all(&target_dir_abs)?;

        let filename = library::unique_filename(&target_dir_abs, &original, asset_id.0);
        let new_abs = target_dir_abs.join(&filename);
        let old_abs = library.absolute(Path::new(&old_rel));
        if old_abs.exists() && old_abs != new_abs {
            // Best-effort: rename, fall back to copy+remove for cross-device moves.
            if std::fs::rename(&old_abs, &new_abs).is_err() {
                std::fs::copy(&old_abs, &new_abs)?;
                let _ = std::fs::remove_file(&old_abs);
            }
        }

        let new_rel = target_dir_rel.join(&filename);
        let new_rel_str = new_rel.to_string_lossy().to_string();
        repository::assets::set_relative_path(pool, asset_id, &new_rel_str).await?;
    }
    Ok(())
}

async fn target_dir_for_object(
    pool: &SqlitePool,
    _library: &Library,
    object_id: i64,
) -> ServiceResult<PathBuf> {
    let oid = crate::domain::ObjectId(object_id);
    let object = repository::objects::get_basic(pool, oid).await?;
    let dir_name = repository::objects::get_dir_name(pool, oid)
        .await?
        .unwrap_or_else(|| library::sanitize_segment(&object.name));
    let parent_chain = match object.folder_id {
        Some(p) => repository::folders::dir_chain(pool, p).await?,
        None => Vec::new(),
    };
    Ok(relative_dir(&parent_chain, &dir_name))
}

fn cleanup_legacy_assets_root(library: &Library) {
    let legacy = library.root().join("assets");
    if !legacy.exists() {
        return;
    }
    // Walk the legacy tree post-order and rmdir empties.
    fn walk(p: &Path, _visited: &mut HashSet<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(p) {
            for e in entries.flatten() {
                let path = e.path();
                if path.is_dir() {
                    walk(&path, _visited);
                    let _ = std::fs::remove_dir(&path);
                }
            }
        }
    }
    let mut visited = HashSet::new();
    walk(&legacy, &mut visited);
    let _ = std::fs::remove_dir(&legacy);
}
