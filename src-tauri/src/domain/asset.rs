use serde::{Deserialize, Serialize};
use specta::Type;

use super::ids::AssetId;
use super::kinds::AssetKind;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Asset {
    pub id: AssetId,
    pub kind: AssetKind,
    pub relative_path: String,
    pub original_filename: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub sha256: Option<String>,
    pub metadata_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}
