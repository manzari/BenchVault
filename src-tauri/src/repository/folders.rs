use std::collections::HashSet;

use sqlx::{Row, SqlitePool};

use crate::db::now_ts;
use crate::domain::{Folder, FolderId};

fn map_row(r: sqlx::sqlite::SqliteRow) -> Folder {
    Folder {
        id: FolderId(r.get::<i64, _>("id")),
        parent_id: r.try_get::<Option<i64>, _>("parent_id").unwrap_or(None).map(FolderId),
        name: r.get("name"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
        deleted_at: r.try_get::<Option<i64>, _>("deleted_at").unwrap_or(None),
    }
}

pub async fn list_active(pool: &SqlitePool) -> sqlx::Result<Vec<Folder>> {
    let rows = sqlx::query("SELECT * FROM folders WHERE deleted_at IS NULL ORDER BY name")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_row).collect())
}

pub async fn create(
    pool: &SqlitePool,
    parent_id: Option<FolderId>,
    name: &str,
    dir_name: &str,
) -> sqlx::Result<Folder> {
    let now = now_ts();
    let parent_db: Option<i64> = parent_id.map(|x| x.0);
    let id = sqlx::query(
        "INSERT INTO folders (parent_id, name, dir_name, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(parent_db)
    .bind(name)
    .bind(dir_name)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?
    .last_insert_rowid();
    get(pool, FolderId(id)).await
}

pub async fn sibling_dir_names(
    pool: &SqlitePool,
    parent_id: Option<FolderId>,
    exclude_folder: Option<FolderId>,
) -> sqlx::Result<HashSet<String>> {
    let rows = if let Some(p) = parent_id {
        sqlx::query(
            "SELECT dir_name FROM folders
             WHERE parent_id = ? AND deleted_at IS NULL AND dir_name IS NOT NULL
             AND (? IS NULL OR id != ?)",
        )
        .bind(p.0)
        .bind(exclude_folder.map(|x| x.0))
        .bind(exclude_folder.map(|x| x.0))
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT dir_name FROM folders
             WHERE parent_id IS NULL AND deleted_at IS NULL AND dir_name IS NOT NULL
             AND (? IS NULL OR id != ?)",
        )
        .bind(exclude_folder.map(|x| x.0))
        .bind(exclude_folder.map(|x| x.0))
        .fetch_all(pool)
        .await?
    };
    Ok(rows.into_iter().map(|r| r.get::<String, _>("dir_name")).collect())
}

/// Same as `sibling_dir_names` but also includes object dir_names that share the parent folder,
/// since folders and objects coexist as siblings on disk.
pub async fn sibling_dir_names_with_objects(
    pool: &SqlitePool,
    parent_id: Option<FolderId>,
    exclude_folder: Option<FolderId>,
    exclude_object: Option<i64>,
) -> sqlx::Result<HashSet<String>> {
    let mut taken = sibling_dir_names(pool, parent_id, exclude_folder).await?;
    let object_rows = if let Some(p) = parent_id {
        sqlx::query(
            "SELECT dir_name FROM objects
             WHERE folder_id = ? AND deleted_at IS NULL AND dir_name IS NOT NULL
             AND (? IS NULL OR id != ?)",
        )
        .bind(p.0)
        .bind(exclude_object)
        .bind(exclude_object)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT dir_name FROM objects
             WHERE folder_id IS NULL AND deleted_at IS NULL AND dir_name IS NOT NULL
             AND (? IS NULL OR id != ?)",
        )
        .bind(exclude_object)
        .bind(exclude_object)
        .fetch_all(pool)
        .await?
    };
    taken.extend(object_rows.into_iter().map(|r| r.get::<String, _>("dir_name")));
    Ok(taken)
}

pub async fn find_by_parent_and_dir(
    pool: &SqlitePool,
    parent_id: Option<FolderId>,
    dir_name: &str,
) -> sqlx::Result<Option<Folder>> {
    let row = if let Some(p) = parent_id {
        sqlx::query(
            "SELECT * FROM folders
             WHERE parent_id = ? AND dir_name = ? AND deleted_at IS NULL",
        )
        .bind(p.0)
        .bind(dir_name)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT * FROM folders
             WHERE parent_id IS NULL AND dir_name = ? AND deleted_at IS NULL",
        )
        .bind(dir_name)
        .fetch_optional(pool)
        .await?
    };
    Ok(row.map(map_row))
}

pub async fn get_dir_name(pool: &SqlitePool, id: FolderId) -> sqlx::Result<Option<String>> {
    let row = sqlx::query("SELECT dir_name FROM folders WHERE id = ?")
        .bind(id.0)
        .fetch_one(pool)
        .await?;
    Ok(row.try_get::<Option<String>, _>("dir_name").unwrap_or(None))
}

pub async fn set_dir_name(pool: &SqlitePool, id: FolderId, dir_name: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE folders SET dir_name = ? WHERE id = ?")
        .bind(dir_name)
        .bind(id.0)
        .execute(pool)
        .await?;
    Ok(())
}

/// Walk up from `id` to the root, returning the dir_name chain ordered root-first.
/// Folders with NULL dir_name contribute nothing — callers are expected to backfill first.
pub async fn dir_chain(pool: &SqlitePool, id: FolderId) -> sqlx::Result<Vec<String>> {
    let mut chain: Vec<String> = Vec::new();
    let mut cur: Option<i64> = Some(id.0);
    let mut seen: HashSet<i64> = HashSet::new();
    while let Some(cid) = cur {
        if !seen.insert(cid) {
            // parent_id cycle in a corrupt DB — stop instead of looping forever
            break;
        }
        let row = sqlx::query("SELECT parent_id, dir_name FROM folders WHERE id = ?")
            .bind(cid)
            .fetch_one(pool)
            .await?;
        if let Ok(Some(d)) = row.try_get::<Option<String>, _>("dir_name") {
            chain.push(d);
        }
        cur = row.try_get::<Option<i64>, _>("parent_id").unwrap_or(None);
    }
    chain.reverse();
    Ok(chain)
}

pub async fn list_descendants(pool: &SqlitePool, root: FolderId) -> sqlx::Result<Vec<FolderId>> {
    let rows = sqlx::query(
        "WITH RECURSIVE sub(id) AS (
           SELECT id FROM folders WHERE parent_id = ?
           UNION ALL
           SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id
         )
         SELECT id FROM sub",
    )
    .bind(root.0)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| FolderId(r.get::<i64, _>("id"))).collect())
}

pub async fn list_active_children(pool: &SqlitePool, parent: FolderId) -> sqlx::Result<Vec<FolderId>> {
    let rows = sqlx::query(
        "SELECT id FROM folders WHERE parent_id = ? AND deleted_at IS NULL",
    )
    .bind(parent.0)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| FolderId(r.get::<i64, _>("id"))).collect())
}

pub async fn list_active_object_children(
    pool: &SqlitePool,
    parent: FolderId,
) -> sqlx::Result<Vec<i64>> {
    let rows = sqlx::query(
        "SELECT id FROM objects WHERE folder_id = ? AND deleted_at IS NULL",
    )
    .bind(parent.0)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.get::<i64, _>("id")).collect())
}

pub async fn get(pool: &SqlitePool, id: FolderId) -> sqlx::Result<Folder> {
    let row = sqlx::query("SELECT * FROM folders WHERE id = ?")
        .bind(id.0)
        .fetch_one(pool)
        .await?;
    Ok(map_row(row))
}

pub async fn rename(pool: &SqlitePool, id: FolderId, name: &str) -> sqlx::Result<Folder> {
    let now = now_ts();
    sqlx::query("UPDATE folders SET name = ?, updated_at = ? WHERE id = ?")
        .bind(name)
        .bind(now)
        .bind(id.0)
        .execute(pool)
        .await?;
    get(pool, id).await
}

pub async fn move_to(
    pool: &SqlitePool,
    id: FolderId,
    new_parent: Option<FolderId>,
) -> sqlx::Result<()> {
    let now = now_ts();
    sqlx::query("UPDATE folders SET parent_id = ?, updated_at = ? WHERE id = ?")
        .bind(new_parent.map(|x| x.0))
        .bind(now)
        .bind(id.0)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn soft_delete(pool: &SqlitePool, id: FolderId) -> sqlx::Result<()> {
    let now = now_ts();
    sqlx::query("UPDATE folders SET deleted_at = ?, updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(now)
        .bind(id.0)
        .execute(pool)
        .await?;
    Ok(())
}
