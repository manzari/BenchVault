//! 3MF (3D Manufacturing Format) extraction. A `.3mf` is an OPC package — a ZIP
//! of XML — so unlike STL/OBJ we unzip first, then stream the core model part
//! (`3D/3dmodel.model`) with `quick-xml`, feeding `<vertex>`/`<triangle>` into the
//! shared [`MeshAccumulator`]. Geometry is converted to millimeters from the
//! model's declared `unit` (the first format that carries real units). Document
//! metadata, object names, an embedded thumbnail, and a slicer project-settings
//! part are pulled in the same pass.
//!
//! Packages written by BambuStudio/OrcaSlicer use the OPC production extension:
//! the core part is a stub whose objects carry `p:path="/3D/Objects/x.model"` and
//! the real meshes live in those separate parts. Any referenced part is streamed
//! into the same accumulator, so geometry lands either way.
//!
//! First cut: every object's mesh is aggregated at identity transform (the common
//! single-object case). Build-item / nested `<component>` transforms are not yet
//! composed — multi-part assemblies may under-count until that lands.

use std::collections::BTreeMap;
use std::io::{BufReader, Read};
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::mesh::{MeshAccumulator, MeshStats};

#[derive(Debug, Clone, Default)]
pub struct ThreeMfParsed {
    pub stats: MeshStats,
    /// The model's declared unit ("millimeter", "inch", …), verbatim.
    pub units: Option<String>,
    /// Document metadata: Title / Designer / Description / LicenseTerms / Application …
    pub metadata: BTreeMap<String, String>,
    pub object_names: Vec<String>,
    /// Embedded preview PNG bytes, if the package carries one.
    pub thumbnail: Option<Vec<u8>>,
    /// Raw text of a slicer project-settings part, if present (PrusaSlicer/Bambu/…).
    pub slicer_config: Option<String>,
}

type Zip = zip::ZipArchive<std::fs::File>;

pub fn parse_path(path: &Path) -> std::io::Result<ThreeMfParsed> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(zip_io_err)?;

    let names: Vec<String> = zip.file_names().map(|s| s.to_string()).collect();
    let model_name = model_part_name(&names);

    let mut acc = MeshAccumulator::default();
    let mut parsed = ThreeMfParsed::default();
    let parts = parse_model(&mut zip, &model_name, &mut acc, &mut parsed)?;

    // Production extension: pull geometry from every referenced part. Names and
    // metadata stay whatever the core part declared, so the sub-parts write into
    // a scratch sink; only their triangles matter.
    for part in parts {
        if part == model_name {
            continue;
        }
        let mut sink = ThreeMfParsed::default();
        let _ = parse_model(&mut zip, &part, &mut acc, &mut sink);
    }
    parsed.stats = acc.finish();

    parsed.thumbnail = read_thumbnail(&mut zip, &names);
    parsed.slicer_config = read_slicer_config(&mut zip, &names);
    Ok(parsed)
}

/// Stream one model part into `acc`, collecting document metadata and object names
/// into `out`. Returns the zip paths of any external model parts it references via
/// the production extension's `p:path`, deduped and in first-seen order.
fn parse_model(
    zip: &mut Zip,
    model_name: &str,
    acc: &mut MeshAccumulator,
    out: &mut ThreeMfParsed,
) -> std::io::Result<Vec<String>> {
    let entry = zip.by_name(model_name).map_err(zip_io_err)?;
    let mut reader = Reader::from_reader(BufReader::new(entry));
    reader.config_mut().trim_text(true);

    let mut parts: Vec<String> = Vec::new();
    let mut scale = 1.0_f64; // to millimeters
    let mut verts: Vec<[f64; 3]> = Vec::new(); // current object's vertices
    let mut meta_name: Option<String> = None; // open <metadata name=..>
    let mut meta_val = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                // `p:path` appears on <object> and on <build><item>; take it from
                // whichever element carries it.
                if let Some(p) = attr(&e, b"path") {
                    let p = p.trim_start_matches('/').to_string();
                    if p.to_ascii_lowercase().ends_with(".model") && !parts.contains(&p) {
                        parts.push(p);
                    }
                }
                match local_name(e.name().as_ref()) {
                    b"model" => {
                        if let Some(u) = attr(&e, b"unit") {
                            scale = unit_scale(&u);
                            out.units = Some(u);
                        }
                    }
                    b"object" => {
                        if let Some(n) = attr(&e, b"name") {
                            out.object_names.push(n);
                        }
                    }
                    b"mesh" => verts.clear(),
                    b"vertex" => {
                        if let (Some(x), Some(y), Some(z)) = (
                            attr_f64(&e, b"x"),
                            attr_f64(&e, b"y"),
                            attr_f64(&e, b"z"),
                        ) {
                            verts.push([x * scale, y * scale, z * scale]);
                        }
                    }
                    b"triangle" => {
                        if let (Some(a), Some(b2), Some(c)) = (
                            attr_usize(&e, b"v1"),
                            attr_usize(&e, b"v2"),
                            attr_usize(&e, b"v3"),
                        ) {
                            if let (Some(&v0), Some(&v1), Some(&v2)) =
                                (verts.get(a), verts.get(b2), verts.get(c))
                            {
                                acc.add_triangle(v0, v1, v2);
                            }
                        }
                    }
                    b"metadata" => {
                        meta_name = attr(&e, b"name");
                        meta_val.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if meta_name.is_some() {
                    if let Ok(s) = t.unescape() {
                        meta_val.push_str(&s);
                    }
                }
            }
            Ok(Event::End(e)) => {
                if local_name(e.name().as_ref()) == b"metadata" {
                    if let Some(name) = meta_name.take() {
                        let v = meta_val.trim();
                        if !name.is_empty() && !v.is_empty() {
                            out.metadata.insert(name, v.to_string());
                        }
                    }
                }
            }
            Err(_) => break, // tolerate malformed tails — keep whatever we accumulated
            _ => {}
        }
        buf.clear();
    }

    Ok(parts)
}

/// Scale factor from a 3MF `unit` value to millimeters. Unknown → 1.0 (treat as mm).
fn unit_scale(unit: &str) -> f64 {
    match unit {
        "micron" => 0.001,
        "millimeter" => 1.0,
        "centimeter" => 10.0,
        "meter" => 1000.0,
        "inch" => 25.4,
        "foot" => 304.8,
        _ => 1.0,
    }
}

/// The core model part name. Conventionally `3D/3dmodel.model`; fall back to the
/// first `*.model` part if the conventional path is absent.
fn model_part_name(names: &[String]) -> String {
    const CONVENTIONAL: &str = "3D/3dmodel.model";
    if names.iter().any(|n| n == CONVENTIONAL) {
        return CONVENTIONAL.to_string();
    }
    names
        .iter()
        .find(|n| n.to_ascii_lowercase().ends_with(".model"))
        .cloned()
        .unwrap_or_else(|| CONVENTIONAL.to_string())
}

/// Find and read an embedded thumbnail PNG: any `*.png` part whose path mentions
/// "thumbnail", else the first PNG under `Metadata/` or `Thumbnails/`.
fn read_thumbnail(zip: &mut Zip, names: &[String]) -> Option<Vec<u8>> {
    let pick = names
        .iter()
        .find(|n| {
            let l = n.to_ascii_lowercase();
            l.ends_with(".png") && l.contains("thumbnail")
        })
        .or_else(|| {
            names.iter().find(|n| {
                let l = n.to_ascii_lowercase();
                l.ends_with(".png") && (l.starts_with("metadata/") || l.starts_with("thumbnails/"))
            })
        })?;
    read_entry(zip, pick)
}

/// Read the first slicer project-settings part (`Metadata/*.config`) as raw text.
fn read_slicer_config(zip: &mut Zip, names: &[String]) -> Option<String> {
    let pick = names.iter().find(|n| {
        let l = n.to_ascii_lowercase();
        l.starts_with("metadata/") && l.ends_with(".config")
    })?;
    let bytes = read_entry(zip, pick)?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_entry(zip: &mut Zip, name: &str) -> Option<Vec<u8>> {
    let mut f = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Strip any XML namespace prefix (`ns:vertex` → `vertex`).
fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

fn attr(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    for a in e.attributes().flatten() {
        if local_name(a.key.as_ref()) == key {
            return a.unescape_value().ok().map(|v| v.into_owned());
        }
    }
    None
}

fn attr_f64(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<f64> {
    attr(e, key).and_then(|v| v.trim().parse().ok())
}

fn attr_usize(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<usize> {
    attr(e, key).and_then(|v| v.trim().parse().ok())
}

fn zip_io_err(e: zip::result::ZipError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    /// A unit cube (2×3×4 box) as a 3MF core model, parameterized by unit and an
    /// optional metadata/thumbnail. Mirrors the OBJ/STL box fixtures.
    fn box_model(unit: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="{unit}" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <metadata name="Title">Widget</metadata>
  <metadata name="Designer">Ada</metadata>
  <resources>
    <object id="1" type="model" name="body">
      <mesh>
        <vertices>
          <vertex x="0" y="0" z="0"/>
          <vertex x="2" y="0" z="0"/>
          <vertex x="2" y="3" z="0"/>
          <vertex x="0" y="3" z="0"/>
          <vertex x="0" y="0" z="4"/>
          <vertex x="2" y="0" z="4"/>
          <vertex x="2" y="3" z="4"/>
          <vertex x="0" y="3" z="4"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="3" v3="2"/>
          <triangle v1="0" v2="2" v3="1"/>
          <triangle v1="4" v2="5" v3="6"/>
          <triangle v1="4" v2="6" v3="7"/>
          <triangle v1="0" v2="1" v3="5"/>
          <triangle v1="0" v2="5" v3="4"/>
          <triangle v1="2" v2="3" v3="7"/>
          <triangle v1="2" v2="7" v3="6"/>
          <triangle v1="1" v2="2" v3="6"/>
          <triangle v1="1" v2="6" v3="5"/>
          <triangle v1="0" v2="4" v3="7"/>
          <triangle v1="0" v2="7" v3="3"/>
        </triangles>
      </mesh>
    </object>
  </resources>
  <build><item objectid="1"/></build>
</model>"#
        )
    }

    /// The BambuStudio/Orca shape: a core part with no geometry whose object
    /// points at an external part via the production extension's `p:path`.
    fn split_model() -> (String, String) {
        let core = r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06">
  <metadata name="Title">Widget</metadata>
  <resources>
    <object id="2" p:path="/3D/Objects/body.model" name="body" type="model"/>
  </resources>
  <build><item objectid="2" p:path="/3D/Objects/body.model"/></build>
</model>"#;
        // The part carries the mesh under its own <model unit=..> root.
        (core.to_string(), box_model("millimeter"))
    }

    struct Extra {
        thumbnail: Option<&'static [u8]>,
        config: Option<&'static str>,
    }

    fn write_3mf(model: &str, extra: Extra) -> tempfile::NamedTempFile {
        let tmp = tempfile::Builder::new().suffix(".3mf").tempfile().unwrap();
        let mut zw = zip::ZipWriter::new(tmp.reopen().unwrap());
        let opts = SimpleFileOptions::default();

        zw.start_file("[Content_Types].xml", opts).unwrap();
        zw.write_all(br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#).unwrap();

        zw.start_file("_rels/.rels", opts).unwrap();
        zw.write_all(br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel" Target="/3D/3dmodel.model"/></Relationships>"#).unwrap();

        zw.start_file("3D/3dmodel.model", opts).unwrap();
        zw.write_all(model.as_bytes()).unwrap();

        if let Some(png) = extra.thumbnail {
            zw.start_file("Metadata/thumbnail.png", opts).unwrap();
            zw.write_all(png).unwrap();
        }
        if let Some(cfg) = extra.config {
            zw.start_file("Metadata/Slic3r_PE.config", opts).unwrap();
            zw.write_all(cfg.as_bytes()).unwrap();
        }

        zw.finish().unwrap();
        tmp
    }

    #[test]
    fn production_extension_geometry_pulled_from_referenced_part() {
        let (core, part) = split_model();
        let tmp = tempfile::Builder::new().suffix(".3mf").tempfile().unwrap();
        let mut zw = zip::ZipWriter::new(tmp.reopen().unwrap());
        let opts = SimpleFileOptions::default();
        zw.start_file("3D/3dmodel.model", opts).unwrap();
        zw.write_all(core.as_bytes()).unwrap();
        zw.start_file("3D/Objects/body.model", opts).unwrap();
        zw.write_all(part.as_bytes()).unwrap();
        zw.finish().unwrap();

        let p = parse_path(tmp.path()).unwrap();
        assert_eq!(p.stats.triangle_count, 12);
        assert_eq!(p.stats.bbox, [2.0, 3.0, 4.0]);
        // Core-part metadata survives; the sub-part must not clobber it.
        assert_eq!(p.metadata.get("Title").map(String::as_str), Some("Widget"));
        assert_eq!(p.object_names, vec!["body"]);
    }

    #[test]
    fn box_geometry_and_metadata_in_millimeters() {
        let tmp = write_3mf(&box_model("millimeter"), Extra { thumbnail: None, config: None });
        let p = parse_path(tmp.path()).unwrap();

        assert_eq!(p.stats.triangle_count, 12);
        assert_eq!(p.stats.vertex_count, 8);
        assert_eq!(p.stats.bbox, [2.0, 3.0, 4.0]);
        assert!((p.stats.volume - 24.0).abs() < 1e-6, "volume {}", p.stats.volume);
        assert!((p.stats.surface_area - 52.0).abs() < 1e-6, "area {}", p.stats.surface_area);
        assert!(p.stats.is_watertight);

        assert_eq!(p.units.as_deref(), Some("millimeter"));
        assert_eq!(p.metadata.get("Title").map(String::as_str), Some("Widget"));
        assert_eq!(p.metadata.get("Designer").map(String::as_str), Some("Ada"));
        assert_eq!(p.object_names, vec!["body"]);
    }

    #[test]
    fn unit_conversion_to_millimeters() {
        let tmp = write_3mf(&box_model("centimeter"), Extra { thumbnail: None, config: None });
        let p = parse_path(tmp.path()).unwrap();
        // cm → mm: dims ×10, volume ×1000, area ×100.
        assert_eq!(p.stats.bbox, [20.0, 30.0, 40.0]);
        assert!((p.stats.volume - 24000.0).abs() < 1e-3, "volume {}", p.stats.volume);
        assert!((p.stats.surface_area - 5200.0).abs() < 1e-3, "area {}", p.stats.surface_area);
        assert_eq!(p.units.as_deref(), Some("centimeter"));
    }

    #[test]
    fn thumbnail_and_slicer_config_returned_when_present() {
        const PNG: &[u8] = b"\x89PNG\r\n\x1a\nFAKE";
        let tmp = write_3mf(
            &box_model("millimeter"),
            Extra { thumbnail: Some(PNG), config: Some("layer_height = 0.2\n") },
        );
        let p = parse_path(tmp.path()).unwrap();
        assert_eq!(p.thumbnail.as_deref(), Some(PNG));
        assert_eq!(p.slicer_config.as_deref(), Some("layer_height = 0.2\n"));
    }

    #[test]
    fn geometry_only_package_yields_no_metadata() {
        let tmp = write_3mf(&box_model("millimeter"), Extra { thumbnail: None, config: None });
        let p = parse_path(tmp.path()).unwrap();
        assert!(p.thumbnail.is_none());
        assert!(p.slicer_config.is_none());
    }
}
