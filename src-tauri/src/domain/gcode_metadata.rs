use serde::{Deserialize, Serialize};
use specta::Type;

use super::ids::AssetId;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GcodeMetadata {
    pub asset_id: AssetId,
    pub slicer_name: Option<String>,
    pub slicer_version: Option<String>,
    pub profile_name: Option<String>,
    pub layer_height_mm: Option<f64>,
    pub nozzle_temp_c: Option<i64>,
    pub bed_temp_c: Option<i64>,
    pub estimated_time_seconds: Option<i64>,
    pub filament_length_mm: Option<f64>,
    pub filament_weight_g: Option<f64>,
    pub filament_type: Option<String>,
    pub parsed_at: Option<i64>,
    pub export_to_usb: bool,
}

impl Default for GcodeMetadata {
    fn default() -> Self {
        Self {
            asset_id: AssetId(0),
            slicer_name: None,
            slicer_version: None,
            profile_name: None,
            layer_height_mm: None,
            nozzle_temp_c: None,
            bed_temp_c: None,
            estimated_time_seconds: None,
            filament_length_mm: None,
            filament_weight_g: None,
            filament_type: None,
            parsed_at: None,
            export_to_usb: true,
        }
    }
}

/// Output of the parser. Extras (anything we couldn't model) are stashed in `assets.metadata_json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct ParsedGcode {
    pub slicer_name: Option<String>,
    pub slicer_version: Option<String>,
    pub profile_name: Option<String>,
    pub layer_height_mm: Option<f64>,
    pub nozzle_temp_c: Option<i64>,
    pub bed_temp_c: Option<i64>,
    pub estimated_time_seconds: Option<i64>,
    pub filament_length_mm: Option<f64>,
    pub filament_weight_g: Option<f64>,
    pub filament_type: Option<String>,
    pub raw_extras: std::collections::HashMap<String, String>,
}
