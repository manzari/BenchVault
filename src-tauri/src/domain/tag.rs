use serde::{Deserialize, Serialize};
use specta::Type;

use super::ids::TagId;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Tag {
    pub id: TagId,
    pub name: String,
    pub color: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SearchHit {
    pub entity_kind: String,
    pub entity_id: i64,
    pub name: String,
    pub snippet: String,
}
