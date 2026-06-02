use std::path::{Path, PathBuf};

use sqlx::SqlitePool;

use crate::domain::{
    AssetId, CreateObjectInput, FolderId, Object, ObjectFull, ObjectId, UpdateObjectPatch,
};
use crate::library::{self, Library};
use crate::repository;

use super::files;
use super::folders;
use super::{ServiceError, ServiceResult};

pub async fn create_with_files(
    pool: &SqlitePool,
    library: &Library,
    input: CreateObjectInput,
) -> ServiceResult<Object> {
    let trimmed_name = input.name.trim();
    let taken = repository::folders::sibling_dir_names_with_objects(
        pool,
        input.folder_id,
        None,
        None,
    )
    .await?;
    let mut tx_object =
        repository::objects::create(pool, input.folder_id, trimmed_name, "").await?;
    let dir_name = library::pick_dir_name(trimmed_name, &taken, tx_object.id.0);
    repository::objects::set_dir_name(pool, tx_object.id, &dir_name).await?;

    // Materialize the object directory now so the on-disk tree mirrors the UI even
    // before any files are added.
    let parent_chain = match input.folder_id {
        Some(p) => repository::folders::dir_chain(pool, p).await?,
        None => Vec::new(),
    };
    let abs = library.absolute(&folders::relative_dir(&parent_chain, &dir_name));
    let _ = std::fs::create_dir_all(&abs);

    // Apply optional fields via update().
    let patch = UpdateObjectPatch {
        notes: input.notes,
        source_url: input.source_url,
        author: input.author,
        license: input.license,
        ..Default::default()
    };
    if patch.notes.is_some() || patch.source_url.is_some() || patch.author.is_some() || patch.license.is_some() {
        tx_object = repository::objects::update(pool, tx_object.id, patch).await?;
    }

    // Ingest the initial files; if any fails, tear the just-created object back
    // down so a failed creation doesn't leave a stray (possibly file-less)
    // object behind for the caller that reported total failure.
    let ingest: ServiceResult<()> = async {
        for stl in &input.stl_paths {
            if stl.is_empty() { continue; }
            files::add_stl(pool, library, tx_object.id, Path::new(stl)).await?;
        }
        if let Some(cover) = input.cover_image_path.as_deref().filter(|s| !s.is_empty()) {
            files::set_cover_image(pool, library, tx_object.id, Path::new(cover)).await?;
        }
        Ok(())
    }
    .await;
    if let Err(e) = ingest {
        let _ = delete(pool, library, tx_object.id).await;
        return Err(e);
    }

    Ok(repository::objects::get_basic(pool, tx_object.id).await?)
}

pub async fn update(
    pool: &SqlitePool,
    library: &Library,
    id: ObjectId,
    patch: UpdateObjectPatch,
) -> ServiceResult<Object> {
    let renaming = patch.name.as_ref().map(|s| s.trim().to_string());
    let updated = repository::objects::update(pool, id, patch).await?;

    if let Some(new_name) = renaming {
        if !new_name.is_empty() {
            apply_rename(pool, library, id, &new_name).await?;
        }
    }
    Ok(updated)
}

pub async fn move_to(
    pool: &SqlitePool,
    library: &Library,
    id: ObjectId,
    new_folder: Option<FolderId>,
) -> ServiceResult<()> {
    let object = repository::objects::get_basic(pool, id).await?;
    let old_parent_chain = match object.folder_id {
        Some(p) => repository::folders::dir_chain(pool, p).await?,
        None => Vec::new(),
    };
    let old_dir_name = repository::objects::get_dir_name(pool, id).await?.unwrap_or_default();
    let new_parent_chain = match new_folder {
        Some(p) => repository::folders::dir_chain(pool, p).await?,
        None => Vec::new(),
    };
    let taken =
        repository::folders::sibling_dir_names_with_objects(pool, new_folder, None, Some(id.0))
            .await?;
    let new_dir_name = library::pick_dir_name(&object.name, &taken, id.0);

    // Move files on disk BEFORE committing the DB row change, so a filesystem
    // rename failure leaves the object row pointing at its still-present files
    // rather than at an empty new location.
    let new_rel = folders::relative_dir(&new_parent_chain, &new_dir_name);
    if !old_dir_name.is_empty() {
        let old_rel = folders::relative_dir(&old_parent_chain, &old_dir_name);
        relocate_dir(pool, library, &old_rel, &new_rel).await?;
    } else {
        let _ = std::fs::create_dir_all(library.absolute(&new_rel));
    }

    repository::objects::move_to(pool, id, new_folder).await?;
    repository::objects::set_dir_name(pool, id, &new_dir_name).await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, library: &Library, id: ObjectId) -> ServiceResult<()> {
    let object = repository::objects::get_basic(pool, id).await?;
    let parent_chain = match object.folder_id {
        Some(p) => repository::folders::dir_chain(pool, p).await?,
        None => Vec::new(),
    };
    let old_dir_name = repository::objects::get_dir_name(pool, id).await?.unwrap_or_default();

    // Tear down attached assets first (DB rows + FTS + links/tags). Otherwise the
    // asset rows outlive the object: sha256 dedupe refuses to re-ingest identical
    // files and stale `asset` hits keep showing up in search after the dir is gone.
    for asset_id in repository::objects::asset_ids_for_object(pool, id).await? {
        files::purge_asset_rows(pool, asset_id).await?;
    }

    repository::objects::soft_delete(pool, id).await?;

    if !old_dir_name.is_empty() {
        let abs = library.absolute(&folders::relative_dir(&parent_chain, &old_dir_name));
        library::remove_dir_all_quiet(&abs);
        if let Some(parent) = abs.parent() {
            library::prune_empty_ancestors(parent, library.root());
        }
    }
    Ok(())
}

pub async fn full(pool: &SqlitePool, id: ObjectId) -> ServiceResult<ObjectFull> {
    Ok(repository::objects::get_full(pool, id).await?)
}

/// Pin which `.3mf` asset is the object's main project (overriding the
/// most-recent default). Stored as a polymorphic `object → asset` link.
pub async fn set_main_project(
    pool: &SqlitePool,
    object_id: ObjectId,
    asset_id: AssetId,
) -> ServiceResult<()> {
    repository::links::set_single(pool, "object", object_id.0, "asset", asset_id.0, "main_project")
        .await?;
    repository::objects::touch(pool, object_id).await?;
    Ok(())
}

/// Resolve the absolute on-disk directory for an object, ensuring it exists and that the object
/// has a populated dir_name. Used by ingest paths.
pub async fn ensure_dir(
    pool: &SqlitePool,
    library: &Library,
    id: ObjectId,
) -> ServiceResult<PathBuf> {
    let object = repository::objects::get_basic(pool, id).await?;
    let parent_chain = match object.folder_id {
        Some(p) => repository::folders::dir_chain(pool, p).await?,
        None => Vec::new(),
    };
    let mut dir_name = repository::objects::get_dir_name(pool, id).await?.unwrap_or_default();
    if dir_name.is_empty() {
        let taken = repository::folders::sibling_dir_names_with_objects(
            pool,
            object.folder_id,
            None,
            Some(id.0),
        )
        .await?;
        dir_name = library::pick_dir_name(&object.name, &taken, id.0);
        repository::objects::set_dir_name(pool, id, &dir_name).await?;
    }
    let rel = folders::relative_dir(&parent_chain, &dir_name);
    let abs = library.absolute(&rel);
    std::fs::create_dir_all(&abs)?;
    Ok(abs)
}

async fn apply_rename(
    pool: &SqlitePool,
    library: &Library,
    id: ObjectId,
    new_name: &str,
) -> ServiceResult<()> {
    let object = repository::objects::get_basic(pool, id).await?;
    let parent_chain = match object.folder_id {
        Some(p) => repository::folders::dir_chain(pool, p).await?,
        None => Vec::new(),
    };
    let old_dir_name = repository::objects::get_dir_name(pool, id).await?.unwrap_or_default();
    let taken =
        repository::folders::sibling_dir_names_with_objects(pool, object.folder_id, None, Some(id.0))
            .await?;
    let new_dir_name = library::pick_dir_name(new_name, &taken, id.0);

    if old_dir_name.is_empty() {
        repository::objects::set_dir_name(pool, id, &new_dir_name).await?;
        let abs = library.absolute(&folders::relative_dir(&parent_chain, &new_dir_name));
        let _ = std::fs::create_dir_all(abs);
        return Ok(());
    }
    if new_dir_name == old_dir_name {
        return Ok(());
    }

    let old_rel = folders::relative_dir(&parent_chain, &old_dir_name);
    let new_rel = folders::relative_dir(&parent_chain, &new_dir_name);
    relocate_dir(pool, library, &old_rel, &new_rel).await?;
    repository::objects::set_dir_name(pool, id, &new_dir_name).await?;
    Ok(())
}

async fn relocate_dir(
    pool: &SqlitePool,
    library: &Library,
    old_rel: &Path,
    new_rel: &Path,
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
        std::fs::rename(&old_abs, &new_abs).map_err(ServiceError::Io)?;
    } else {
        std::fs::create_dir_all(&new_abs)?;
    }
    let old_prefix = old_rel.to_string_lossy().to_string();
    let new_prefix = new_rel.to_string_lossy().to_string();
    repository::assets::relocate_prefix(pool, &old_prefix, &new_prefix).await?;
    Ok(())
}

#[allow(dead_code)]
pub fn _suppress_unused_error(e: ServiceError) { drop(e); }
