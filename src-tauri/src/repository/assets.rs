use sqlx::{Row, SqlitePool};

use crate::db::now_ts;
use crate::domain::{Asset, AssetId, AssetKind};

fn map_row(r: sqlx::sqlite::SqliteRow) -> Asset {
    Asset {
        id: AssetId(r.get::<i64, _>("id")),
        kind: AssetKind::from_str(r.get::<&str, _>("kind")),
        relative_path: r.get("relative_path"),
        original_filename: r.get("original_filename"),
        mime_type: r.try_get("mime_type").ok(),
        size_bytes: r.try_get("size_bytes").ok(),
        sha256: r.try_get("sha256").ok(),
        metadata_json: r.try_get("metadata_json").ok(),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
        deleted_at: r.try_get("deleted_at").ok(),
    }
}

pub struct NewAsset<'a> {
    pub kind: AssetKind,
    pub original_filename: &'a str,
    pub mime_type: Option<&'a str>,
}

/// Allocates a row with placeholder relative_path; caller fills it via `set_relative_path`
/// after the file has been copied (so the path can include the row id).
pub async fn allocate(pool: &SqlitePool, new: NewAsset<'_>) -> sqlx::Result<Asset> {
    let now = now_ts();
    let id = sqlx::query(
        "INSERT INTO assets (kind, relative_path, original_filename, mime_type, created_at, updated_at)
         VALUES (?, '', ?, ?, ?, ?)",
    )
    .bind(new.kind.as_str())
    .bind(new.original_filename)
    .bind(new.mime_type)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?
    .last_insert_rowid();
    get(pool, AssetId(id)).await
}

pub async fn finalize(
    pool: &SqlitePool,
    id: AssetId,
    relative_path: &str,
    size_bytes: i64,
    sha256: Option<&str>,
    metadata_json: Option<&str>,
) -> sqlx::Result<Asset> {
    let now = now_ts();
    sqlx::query(
        "UPDATE assets SET relative_path = ?, size_bytes = ?, sha256 = ?, metadata_json = ?, updated_at = ? WHERE id = ?",
    )
    .bind(relative_path)
    .bind(size_bytes)
    .bind(sha256)
    .bind(metadata_json)
    .bind(now)
    .bind(id.0)
    .execute(pool)
    .await?;
    get(pool, id).await
}

pub async fn get(pool: &SqlitePool, id: AssetId) -> sqlx::Result<Asset> {
    let row = sqlx::query("SELECT * FROM assets WHERE id = ?")
        .bind(id.0)
        .fetch_one(pool)
        .await?;
    Ok(map_row(row))
}

pub async fn find_by_relative_path(
    pool: &SqlitePool,
    relative_path: &str,
) -> sqlx::Result<Option<Asset>> {
    let row = sqlx::query(
        "SELECT * FROM assets WHERE relative_path = ? AND deleted_at IS NULL",
    )
    .bind(relative_path)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(map_row))
}

pub async fn find_by_sha256(pool: &SqlitePool, sha256: &str) -> sqlx::Result<Option<Asset>> {
    let row = sqlx::query(
        "SELECT * FROM assets WHERE sha256 = ? AND deleted_at IS NULL",
    )
    .bind(sha256)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(map_row))
}

pub async fn update_path_and_size(
    pool: &SqlitePool,
    id: AssetId,
    relative_path: &str,
    original_filename: &str,
    size_bytes: i64,
    sha256: Option<&str>,
) -> sqlx::Result<()> {
    let now = now_ts();
    sqlx::query(
        "UPDATE assets
            SET relative_path = ?, original_filename = ?, size_bytes = ?, sha256 = ?, updated_at = ?
          WHERE id = ?",
    )
    .bind(relative_path)
    .bind(original_filename)
    .bind(size_bytes)
    .bind(sha256)
    .bind(now)
    .bind(id.0)
    .execute(pool)
    .await?;
    Ok(())
}

/// Refresh just the byte size + content hash after rewriting an asset's file in
/// place (e.g. stripping a gcode line). The path and filename are unchanged.
pub async fn update_size_sha(
    pool: &SqlitePool,
    id: AssetId,
    size_bytes: i64,
    sha256: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE assets SET size_bytes = ?, sha256 = ?, updated_at = ? WHERE id = ?")
        .bind(size_bytes)
        .bind(sha256)
        .bind(now_ts())
        .bind(id.0)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn move_to_object(
    pool: &SqlitePool,
    asset_id: AssetId,
    new_object_id: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE object_assets SET object_id = ? WHERE asset_id = ?",
    )
    .bind(new_object_id)
    .bind(asset_id.0)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_hard(pool: &SqlitePool, id: AssetId) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM assets WHERE id = ?")
        .bind(id.0)
        .execute(pool)
        .await?;
    Ok(())
}

/// Rewrite every asset whose `relative_path` is under `old_prefix` to live under `new_prefix`.
/// Both prefixes are directory paths (no trailing slash); the swap is `<prefix>/...` → `<prefix>/...`.
pub async fn relocate_prefix(
    pool: &SqlitePool,
    old_prefix: &str,
    new_prefix: &str,
) -> sqlx::Result<()> {
    let now = now_ts();
    let old_with_sep = format!("{old_prefix}/");
    let new_with_sep = format!("{new_prefix}/");
    // substr comparison instead of LIKE: avoids `_`/`%` wildcards and ASCII
    // case-insensitivity rewriting paths of unrelated sibling directories.
    sqlx::query(
        "UPDATE assets
         SET relative_path = ? || substr(relative_path, length(?) + 1),
             updated_at = ?
         WHERE substr(relative_path, 1, length(?)) = ?",
    )
    .bind(&new_with_sep)
    .bind(&old_with_sep)
    .bind(now)
    .bind(&old_with_sep)
    .bind(&old_with_sep)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_all_paths(pool: &SqlitePool) -> sqlx::Result<Vec<(AssetId, String)>> {
    let rows = sqlx::query("SELECT id, relative_path FROM assets")
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| (AssetId(r.get::<i64, _>("id")), r.get::<String, _>("relative_path")))
        .collect())
}

pub async fn set_relative_path(
    pool: &SqlitePool,
    id: AssetId,
    relative_path: &str,
) -> sqlx::Result<()> {
    let now = now_ts();
    sqlx::query("UPDATE assets SET relative_path = ?, updated_at = ? WHERE id = ?")
        .bind(relative_path)
        .bind(now)
        .bind(id.0)
        .execute(pool)
        .await?;
    Ok(())
}
