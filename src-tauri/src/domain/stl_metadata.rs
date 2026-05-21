use serde::{Deserialize, Serialize};
use specta::Type;

use super::ids::AssetId;

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct StlMetadata {
    pub asset_id: AssetId,
    pub triangle_count: Option<i64>,
    pub bbox_x_mm: Option<f64>,
    pub bbox_y_mm: Option<f64>,
    pub bbox_z_mm: Option<f64>,
}
