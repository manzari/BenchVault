use serde::{Deserialize, Serialize};
use specta::Type;

use super::ids::AssetId;

/// Computed geometry for a mesh-bearing asset (STL now; OBJ/3MF/SCAD-render later).
/// Distances are in the file's own units — millimeters for STL/OBJ, whatever
/// `source_units` declares for 3MF.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct MeshMetadata {
    pub asset_id: AssetId,
    pub source_format: Option<String>,
    pub source_units: Option<String>,
    pub triangle_count: Option<i64>,
    pub vertex_count: Option<i64>,
    pub bbox_x_mm: Option<f64>,
    pub bbox_y_mm: Option<f64>,
    pub bbox_z_mm: Option<f64>,
    pub volume_mm3: Option<f64>,
    pub surface_area_mm2: Option<f64>,
    pub is_watertight: Option<bool>,
    pub parsed_at: Option<i64>,
}
