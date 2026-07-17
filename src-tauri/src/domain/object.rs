use serde::{Deserialize, Serialize};
use specta::Type;

use super::asset::Asset;
use super::gcode_metadata::GcodeMetadata;
use super::mesh_metadata::MeshMetadata;
use super::ids::{AssetId, FolderId, ObjectId};
use super::kinds::ObjectAssetRole;
use super::tag::Tag;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Object {
    pub id: ObjectId,
    pub folder_id: Option<FolderId>,
    pub name: String,
    pub notes: Option<String>,
    pub source_url: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub cover_asset_id: Option<AssetId>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ObjectSummary {
    pub id: ObjectId,
    pub folder_id: Option<FolderId>,
    pub name: String,
    pub cover_asset_id: Option<AssetId>,
    pub stl_count: i64,
    pub gcode_count: i64,
    pub updated_at: i64,
}

/// An asset attached to an object plus role + sort info plus type-specific metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AttachedAsset {
    pub asset: Asset,
    pub role: ObjectAssetRole,
    pub sort_order: i64,
    pub gcode_metadata: Option<GcodeMetadata>,
    pub mesh_metadata: Option<MeshMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ObjectFull {
    pub object: Object,
    pub assets: Vec<AttachedAsset>,
    pub tags: Vec<Tag>,
    /// The object's "main" `.3mf` project: the user's explicit pick if set,
    /// otherwise the most recently updated `.3mf`. `None` if the object has no
    /// `.3mf` assets.
    pub main_project_asset_id: Option<AssetId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CreateObjectInput {
    pub name: String,
    pub folder_id: Option<FolderId>,
    pub notes: Option<String>,
    pub source_url: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub stl_paths: Vec<String>,
    pub cover_image_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct UpdateObjectPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}
