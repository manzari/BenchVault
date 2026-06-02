use std::path::PathBuf;

use sqlx::SqlitePool;

use crate::domain::{Folder, FolderId};
use crate::library::{self, Library};
use crate::repository;

use super::{ServiceError, ServiceResult};

/// Reject moving a folder into itself or one of its descendants — that would
/// write a parent_id cycle, after which every dir_chain walk loops forever.
async fn ensure_not_cyclic(
    pool: &SqlitePool,
    id: FolderId,
    new_parent: Option<FolderId>,
) -> ServiceResult<()> {
    let mut cur = new_parent;
    while let Some(p) = cur {
        if p == id {
            return Err(ServiceError::Validation(
                "cannot move a folder into itself or one of its descendants".into(),
            ));
        }
        cur = repository::folders::get(pool, p).await?.parent_id;
    }
    Ok(())
}

pub async fn create(
    pool: &SqlitePool,
    library: &Library,
    parent_id: Option<FolderId>,
    name: &str,
) -> ServiceResult<Folder> {
    let taken =
        repository::folders::sibling_dir_names_with_objects(pool, parent_id, None, None).await?;
    // The id isn't known until insert; insert with a temporary placeholder, then patch dir_name.
    let folder = repository::folders::create(pool, parent_id, name, "").await?;
    let dir_name = library::pick_dir_name(name, &taken, folder.id.0);
    repository::folders::set_dir_name(pool, folder.id, &dir_name).await?;

    let parent_chain = match parent_id {
        Some(p) => repository::folders::dir_chain(pool, p).await?,
        None => Vec::new(),
    };
    let abs = library.absolute(&relative_dir(&parent_chain, &dir_name));
    let _ = std::fs::create_dir_all(&abs);

    Ok(Folder { ..folder })
}

pub async fn rename(
    pool: &SqlitePool,
    library: &Library,
    id: FolderId,
    new_name: &str,
) -> ServiceResult<Folder> {
    let folder = repository::folders::get(pool, id).await?;
    let parent_chain = match folder.parent_id {
        Some(p) => repository::folders::dir_chain(pool, p).await?,
        None => Vec::new(),
    };
    let old_dir_name = repository::folders::get_dir_name(pool, id).await?.unwrap_or_default();
    let taken =
        repository::folders::sibling_dir_names_with_objects(pool, folder.parent_id, Some(id), None)
            .await?;
    let new_dir_name = library::pick_dir_name(new_name, &taken, id.0);

    let updated = repository::folders::rename(pool, id, new_name).await?;

    if !old_dir_name.is_empty() && new_dir_name != old_dir_name {
        let old_rel = relative_dir(&parent_chain, &old_dir_name);
        let new_rel = relative_dir(&parent_chain, &new_dir_name);
        relocate_dir(pool, library, &old_rel, &new_rel).await?;
        repository::folders::set_dir_name(pool, id, &new_dir_name).await?;
    } else if old_dir_name.is_empty() {
        repository::folders::set_dir_name(pool, id, &new_dir_name).await?;
        let abs = library.absolute(&relative_dir(&parent_chain, &new_dir_name));
        let _ = std::fs::create_dir_all(abs);
    }

    Ok(updated)
}

pub async fn move_to(
    pool: &SqlitePool,
    library: &Library,
    id: FolderId,
    new_parent: Option<FolderId>,
) -> ServiceResult<()> {
    ensure_not_cyclic(pool, id, new_parent).await?;
    let folder = repository::folders::get(pool, id).await?;
    let old_parent_chain = match folder.parent_id {
        Some(p) => repository::folders::dir_chain(pool, p).await?,
        None => Vec::new(),
    };
    let old_dir_name = repository::folders::get_dir_name(pool, id).await?.unwrap_or_default();
    let new_parent_chain = match new_parent {
        Some(p) => repository::folders::dir_chain(pool, p).await?,
        None => Vec::new(),
    };
    let taken =
        repository::folders::sibling_dir_names_with_objects(pool, new_parent, Some(id), None)
            .await?;
    let new_dir_name = library::pick_dir_name(&folder.name, &taken, id.0);

    // Move files on disk BEFORE committing the DB row change. A filesystem
    // rename failure then leaves the DB untouched, rather than pointing the
    // folder row at a location whose files were never moved.
    if !old_dir_name.is_empty() {
        let old_rel = relative_dir(&old_parent_chain, &old_dir_name);
        let new_rel = relative_dir(&new_parent_chain, &new_dir_name);
        relocate_dir(pool, library, &old_rel, &new_rel).await?;
    } else {
        let abs = library.absolute(&relative_dir(&new_parent_chain, &new_dir_name));
        let _ = std::fs::create_dir_all(abs);
    }

    repository::folders::move_to(pool, id, new_parent).await?;
    repository::folders::set_dir_name(pool, id, &new_dir_name).await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, library: &Library, id: FolderId) -> ServiceResult<()> {
    // Match the UI's "objects inside drop to root" promise: reparent direct children to root
    // before tearing down the folder's on-disk directory. Recursive grandchildren ride along
    // because each child's `move_to` renames its dir wholesale.
    let child_folders = repository::folders::list_active_children(pool, id).await?;
    for cid in child_folders {
        move_to(pool, library, cid, None).await?;
    }
    let child_objects = repository::folders::list_active_object_children(pool, id).await?;
    for oid in child_objects {
        super::objects::move_to(pool, library, crate::domain::ObjectId(oid), None).await?;
    }

    let folder = repository::folders::get(pool, id).await?;
    let parent_chain = match folder.parent_id {
        Some(p) => repository::folders::dir_chain(pool, p).await?,
        None => Vec::new(),
    };
    let old_dir_name = repository::folders::get_dir_name(pool, id).await?.unwrap_or_default();

    repository::folders::soft_delete(pool, id).await?;

    if !old_dir_name.is_empty() {
        let abs = library.absolute(&relative_dir(&parent_chain, &old_dir_name));
        library::remove_dir_all_quiet(&abs);
        if let Some(parent) = abs.parent() {
            library::prune_empty_ancestors(parent, library.root());
        }
    }
    Ok(())
}

pub fn relative_dir(parent_chain: &[String], dir_name: &str) -> PathBuf {
    let mut p = PathBuf::new();
    for seg in parent_chain {
        p.push(seg);
    }
    p.push(dir_name);
    p
}

async fn relocate_dir(
    pool: &SqlitePool,
    library: &Library,
    old_rel: &std::path::Path,
    new_rel: &std::path::Path,
) -> ServiceResult<()> {
    let old_abs = library.absolute(old_rel);
    let new_abs = library.absolute(new_rel);
    if old_abs == new_abs {
        return Ok(());
    }
    if let Some(parent) = new_abs.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if old_abs.exists() {
        std::fs::rename(&old_abs, &new_abs)?;
    } else {
        std::fs::create_dir_all(&new_abs)?;
    }
    let old_prefix = old_rel.to_string_lossy().to_string();
    let new_prefix = new_rel.to_string_lossy().to_string();
    repository::assets::relocate_prefix(pool, &old_prefix, &new_prefix).await?;
    Ok(())
}
