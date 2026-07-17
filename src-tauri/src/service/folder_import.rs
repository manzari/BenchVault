//! Import a directory tree as an object: walks recursively, classifies files, and routes
//! each one through the existing single-file ingest path (`service::files::add_*`).
//!
//! Used by the drag-and-drop flow when a user drops a folder onto the window. The folder's
//! basename becomes the object name; subdirectories are flattened into the object's library
//! directory because the on-disk layout is `<library>/<folder-chain>/<object-dir>/<files>`.

use std::path::{Path, PathBuf};

use serde::Serialize;
use specta::Type;
use sqlx::SqlitePool;

use crate::domain::{AssetId, CreateObjectInput, FolderId, Object, ObjectId};
use crate::library::Library;
use crate::repository;

use super::{files, objects, ServiceError, ServiceResult};

#[derive(Debug, Clone, Serialize, Type)]
pub struct FolderImportReport {
    pub object: Object,
    pub stl_count: u32,
    pub gcode_count: u32,
    pub image_count: u32,
    pub other_count: u32,
    pub skipped_count: u32,
}

#[derive(Debug, Clone, Default)]
struct Counts {
    stl: u32,
    gcode: u32,
    image: u32,
    other: u32,
    skipped: u32,
}

pub async fn import_folder_as_object(
    pool: &SqlitePool,
    library: &Library,
    source: &Path,
    parent_folder_id: Option<FolderId>,
) -> ServiceResult<FolderImportReport> {
    let resolved = source.canonicalize().unwrap_or_else(|_| source.to_path_buf());
    if !resolved.is_dir() {
        return Err(ServiceError::Other(format!(
            "not a directory: {}",
            resolved.display()
        )));
    }
    let name = resolved
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Imported folder".to_string());

    let object = objects::create_with_files(
        pool,
        library,
        CreateObjectInput {
            name,
            folder_id: parent_folder_id,
            notes: None,
            source_url: None,
            author: None,
            license: None,
            stl_paths: Vec::new(),
            cover_image_path: None,
        },
    )
    .await?;

    let counts = ingest_folder(pool, library, object.id, &resolved).await?;
    let object = repository::objects::get_basic(pool, object.id).await?;
    Ok(report(object, counts))
}

pub async fn import_folder_into_object(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    source: &Path,
) -> ServiceResult<FolderImportReport> {
    let resolved = source.canonicalize().unwrap_or_else(|_| source.to_path_buf());
    if !resolved.is_dir() {
        return Err(ServiceError::Other(format!(
            "not a directory: {}",
            resolved.display()
        )));
    }
    let counts = ingest_folder(pool, library, object_id, &resolved).await?;
    let object = repository::objects::get_basic(pool, object_id).await?;
    Ok(report(object, counts))
}

async fn ingest_folder(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    source: &Path,
) -> ServiceResult<Counts> {
    let mut counts = Counts::default();
    let mut paths: Vec<PathBuf> = Vec::new();
    walk(source, &mut paths)?;
    paths.sort();

    let mut first_image: Option<AssetId> = None;
    for path in paths {
        let name_lower = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if is_junk(&name_lower) {
            counts.skipped += 1;
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "stl" | "3mf" | "obj" | "step" | "stp" => match files::add_stl(pool, library, object_id, &path).await {
                Ok(_) => counts.stl += 1,
                Err(e) => {
                    eprintln!("[folder_import] STL '{}': {e}", path.display());
                    counts.skipped += 1;
                }
            },
            "gcode" | "bgcode" | "gco" | "g" => match files::add_gcode(pool, library, object_id, &path).await {
                Ok(_) => counts.gcode += 1,
                Err(e) => {
                    eprintln!("[folder_import] gcode '{}': {e}", path.display());
                    counts.skipped += 1;
                }
            },
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" => {
                match files::add_attachment(pool, library, object_id, &path).await {
                    Ok(asset) => {
                        counts.image += 1;
                        if first_image.is_none() {
                            first_image = Some(asset.id);
                        }
                    }
                    Err(e) => {
                        eprintln!("[folder_import] image '{}': {e}", path.display());
                        counts.skipped += 1;
                    }
                }
            }
            _ => match files::add_attachment(pool, library, object_id, &path).await {
                Ok(_) => counts.other += 1,
                Err(e) => {
                    eprintln!("[folder_import] file '{}': {e}", path.display());
                    counts.skipped += 1;
                }
            },
        }
    }

    if let Some(asset_id) = first_image {
        repository::objects::set_cover_if_missing(pool, object_id, asset_id).await?;
    }
    repository::objects::touch(pool, object_id).await?;
    Ok(counts)
}

fn walk(root: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_s = name.to_string_lossy();
            if name_s.starts_with('.') || name_s == "__MACOSX" {
                continue;
            }
            // Skip symlinks defensively — avoids cycles and surprises with broken links.
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if ft.is_symlink() {
                continue;
            }
            let path = entry.path();
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                out.push(path);
            }
        }
    }
    Ok(())
}

fn is_junk(name_lower: &str) -> bool {
    matches!(name_lower, "thumbs.db" | "desktop.ini")
}

fn report(object: Object, counts: Counts) -> FolderImportReport {
    FolderImportReport {
        object,
        stl_count: counts.stl,
        gcode_count: counts.gcode,
        image_count: counts.image,
        other_count: counts.other,
        skipped_count: counts.skipped,
    }
}
