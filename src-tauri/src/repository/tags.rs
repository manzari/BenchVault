use sqlx::{Row, SqlitePool};

use crate::db::now_ts;
use crate::domain::{Tag, TagId};

fn map_row(r: sqlx::sqlite::SqliteRow) -> Tag {
    Tag {
        id: TagId(r.get::<i64, _>("id")),
        name: r.get("name"),
        color: r.try_get("color").ok(),
        created_at: r.get("created_at"),
    }
}

pub async fn list(pool: &SqlitePool) -> sqlx::Result<Vec<Tag>> {
    let rows = sqlx::query("SELECT * FROM tags ORDER BY name")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_row).collect())
}

pub async fn create(pool: &SqlitePool, name: &str, color: Option<&str>) -> sqlx::Result<Tag> {
    let now = now_ts();
    let id = sqlx::query("INSERT INTO tags (name, color, created_at) VALUES (?, ?, ?)")
        .bind(name)
        .bind(color)
        .bind(now)
        .execute(pool)
        .await?
        .last_insert_rowid();
    let row = sqlx::query("SELECT * FROM tags WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(map_row(row))
}

pub async fn attach(
    pool: &SqlitePool,
    entity_kind: &str,
    entity_id: i64,
    tag_id: TagId,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO entity_tags (entity_kind, entity_id, tag_id) VALUES (?, ?, ?)",
    )
    .bind(entity_kind)
    .bind(entity_id)
    .bind(tag_id.0)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn detach(
    pool: &SqlitePool,
    entity_kind: &str,
    entity_id: i64,
    tag_id: TagId,
) -> sqlx::Result<()> {
    sqlx::query(
        "DELETE FROM entity_tags WHERE entity_kind = ? AND entity_id = ? AND tag_id = ?",
    )
    .bind(entity_kind)
    .bind(entity_id)
    .bind(tag_id.0)
    .execute(pool)
    .await?;
    Ok(())
}

/// Drop every tag association for an entity, so deleting it doesn't leave
/// dangling `entity_tags` rows (which could later be inherited by a reused id).
pub async fn delete_for_entity(
    pool: &SqlitePool,
    entity_kind: &str,
    entity_id: i64,
) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM entity_tags WHERE entity_kind = ? AND entity_id = ?")
        .bind(entity_kind)
        .bind(entity_id)
        .execute(pool)
        .await?;
    Ok(())
}
