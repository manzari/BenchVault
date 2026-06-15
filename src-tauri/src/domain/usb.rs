use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MountedDrive {
    pub mount_path: String,
    pub label: Option<String>,
    pub fs_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct UsbTarget {
    pub uuid: String,
    pub label: String,
    pub registered_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbMarker {
    pub uuid: String,
    pub label: String,
    pub created_at: i64,
    pub schema_version: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsbManifest {
    #[serde(default)]
    pub files: Vec<UsbManifestFile>,
    #[serde(default)]
    pub dirs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbManifestFile {
    pub relative_path: String,
    pub size: i64,
    pub asset_id: i64,
    /// `assets.updated_at` of the library copy when it was last mirrored. `None` on
    /// manifests from older versions or adopted files — treated as "trust size match".
    #[serde(default)]
    pub synced_updated_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct SyncReport {
    pub copied: u32,
    pub updated: u32,
    pub deleted: u32,
    pub skipped: bool,
    pub reason: Option<String>,
    pub errors: Vec<String>,
    pub started_at: i64,
    pub finished_at: i64,
}
