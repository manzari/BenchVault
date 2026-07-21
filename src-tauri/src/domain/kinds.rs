use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Stl,
    Gcode,
    Image,
    Cad,
    Scad,
    Obj,
    Step,
    // Keep the wire/DB string as one word ("cadquery"), not snake_case's
    // "cad_query", so serde output matches `as_str`/`from_str` and the stored value.
    #[serde(rename = "cadquery")]
    CadQuery,
    // snake_case would emit "three_mf"; keep the wire/DB string one word to match
    // `as_str`/`from_str` and the stored value (mirrors `CadQuery` → "cadquery").
    #[serde(rename = "threemf")]
    ThreeMf,
    Markdown,
    Text,
    Other,
}

impl AssetKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetKind::Stl => "stl",
            AssetKind::Gcode => "gcode",
            AssetKind::Image => "image",
            AssetKind::Cad => "cad",
            AssetKind::Scad => "scad",
            AssetKind::Obj => "obj",
            AssetKind::Step => "step",
            AssetKind::CadQuery => "cadquery",
            AssetKind::ThreeMf => "threemf",
            AssetKind::Markdown => "markdown",
            AssetKind::Text => "text",
            AssetKind::Other => "other",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "stl" => AssetKind::Stl,
            "gcode" => AssetKind::Gcode,
            "image" => AssetKind::Image,
            "cad" => AssetKind::Cad,
            "scad" => AssetKind::Scad,
            "obj" => AssetKind::Obj,
            "step" => AssetKind::Step,
            "cadquery" => AssetKind::CadQuery,
            "threemf" => AssetKind::ThreeMf,
            "markdown" => AssetKind::Markdown,
            "text" => AssetKind::Text,
            _ => AssetKind::Other,
        }
    }
}

/// Polymorphic entity kind used by tags, links, and the FTS index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Object,
    Folder,
    Asset,
    // Reserved for future entities — adding a variant here doesn't change SQL.
}

impl EntityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityKind::Object => "object",
            EntityKind::Folder => "folder",
            EntityKind::Asset => "asset",
        }
    }
}

/// Role of an asset within an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ObjectAssetRole {
    StlPart,
    Gcode,
    Scad,
    Document,
    Attachment,
}

impl ObjectAssetRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            ObjectAssetRole::StlPart => "stl_part",
            ObjectAssetRole::Gcode => "gcode",
            ObjectAssetRole::Scad => "scad",
            ObjectAssetRole::Document => "document",
            ObjectAssetRole::Attachment => "attachment",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "stl_part" => ObjectAssetRole::StlPart,
            "gcode" => ObjectAssetRole::Gcode,
            "scad" => ObjectAssetRole::Scad,
            "document" => ObjectAssetRole::Document,
            _ => ObjectAssetRole::Attachment,
        }
    }
}
