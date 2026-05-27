use sqlx::SqlitePool;

use crate::db::now_ts;

pub async fn create(
    pool: &SqlitePool,
    from_kind: &str,
    from_id: i64,
    to_kind: &str,
    to_id: i64,
    relation: &str,
    metadata_json: Option<&str>,
) -> sqlx::Result<()> {
    let now = now_ts();
    sqlx::query(
        "INSERT OR IGNORE INTO entity_links
           (from_kind, from_id, to_kind, to_id, relation, metadata_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(from_kind)
    .bind(from_id)
    .bind(to_kind)
    .bind(to_id)
    .bind(relation)
    .bind(metadata_json)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Set a relation that must have at most one target per source (e.g. an object's
/// "main_project" asset): clear any existing links of this `relation` from the
/// source, then insert the new one.
pub async fn set_single(
    pool: &SqlitePool,
    from_kind: &str,
    from_id: i64,
    to_kind: &str,
    to_id: i64,
    relation: &str,
) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "DELETE FROM entity_links WHERE from_kind = ? AND from_id = ? AND relation = ?",
    )
    .bind(from_kind)
    .bind(from_id)
    .bind(relation)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO entity_links
           (from_kind, from_id, to_kind, to_id, relation, metadata_json, created_at)
         VALUES (?, ?, ?, ?, ?, NULL, ?)",
    )
    .bind(from_kind)
    .bind(from_id)
    .bind(to_kind)
    .bind(to_id)
    .bind(relation)
    .bind(now_ts())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Remove every link that touches an asset (either endpoint), so deleting an
/// asset doesn't leave dangling references (e.g. a stale "main_project").
pub async fn delete_for_asset(pool: &SqlitePool, asset_id: i64) -> sqlx::Result<()> {
    sqlx::query(
        "DELETE FROM entity_links
         WHERE (to_kind = 'asset' AND to_id = ?) OR (from_kind = 'asset' AND from_id = ?)",
    )
    .bind(asset_id)
    .bind(asset_id)
    .execute(pool)
    .await?;
    Ok(())
}
