use serde::{Deserialize, Serialize};
use specta::Type;

use super::ids::FolderId;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Folder {
    pub id: FolderId,
    pub parent_id: Option<FolderId>,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}
