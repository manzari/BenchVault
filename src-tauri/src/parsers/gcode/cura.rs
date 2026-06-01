use once_cell::sync::Lazy;
use regex::Regex;

use crate::domain::ParsedGcode;
use super::common::{capture_first, parse_duration_to_seconds};

static GENERATED_BY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?im)^;Generated with\s+(Cura\S*)\s*([\d\.]+)?").unwrap());
static TIME_LINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?im)^;TIME:\s*(\d+)").unwrap());
static FILAMENT_USED: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?im)^;Filament used:\s*([\d\.,]+)\s*m").unwrap());
static LAYER_HEIGHT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?im)^;Layer height:\s*([\d\.]+)").unwrap());
static NOZZLE_TEMP: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?im)^M104\s+S(\d+)").unwrap());
static BED_TEMP: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?im)^M140\s+S(\d+)").unwrap());

pub fn parse(blob: &str) -> ParsedGcode {
    let mut p = ParsedGcode::default();
    if let Some(cap) = GENERATED_BY.captures(blob) {
        p.slicer_name = cap.get(1).map(|m| m.as_str().to_string());
        p.slicer_version = cap.get(2).map(|m| m.as_str().to_string()).filter(|s| !s.is_empty());
    }
    p.estimated_time_seconds = capture_first(&TIME_LINE, blob).and_then(|s| parse_duration_to_seconds(&s));
    // Cura reports filament in metres → convert to mm.
    if let Some(meters) = capture_first(&FILAMENT_USED, blob)
        .and_then(|s| s.replace(',', "").parse::<f64>().ok())
    {
        p.filament_length_mm = Some(meters * 1000.0);
    }
    p.layer_height_mm = capture_first(&LAYER_HEIGHT, blob).and_then(|s| s.parse::<f64>().ok());
    p.nozzle_temp_c = capture_first(&NOZZLE_TEMP, blob).and_then(|s| s.parse::<i64>().ok());
    p.bed_temp_c = capture_first(&BED_TEMP, blob).and_then(|s| s.parse::<i64>().ok());
    p
}
