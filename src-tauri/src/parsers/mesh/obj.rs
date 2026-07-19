//! Wavefront OBJ geometry extraction. Only geometric vertices (`v`) and faces
//! (`f`) are read; faces are fan-triangulated and vertices are welded by position
//! (see [`MeshAccumulator`]) so shared edges register for the watertight test.
//! Like STL, OBJ carries no units — distances are taken as millimeters.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::{MeshAccumulator, MeshStats};

pub fn parse_path(path: &Path) -> std::io::Result<MeshStats> {
    let f = BufReader::new(File::open(path)?);
    let mut verts: Vec<[f64; 3]> = Vec::new();
    let mut acc = MeshAccumulator::default();

    for line in f.lines() {
        let line = line?;
        let mut toks = line.split_whitespace();
        match toks.next() {
            Some("v") => {
                let mut it = toks.filter_map(|t| t.parse::<f64>().ok());
                if let (Some(x), Some(y), Some(z)) = (it.next(), it.next(), it.next()) {
                    verts.push([x, y, z]);
                }
            }
            Some("f") => {
                // Take the vertex index (before the first '/') of each face token.
                let idxs: Vec<usize> = toks
                    .filter_map(|tok| tok.split('/').next())
                    .filter_map(|s| s.parse::<i64>().ok())
                    .filter_map(|i| resolve(i, verts.len()))
                    .collect();
                if idxs.len() >= 3 {
                    for i in 1..idxs.len() - 1 {
                        acc.add_triangle(verts[idxs[0]], verts[idxs[i]], verts[idxs[i + 1]]);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(acc.finish())
}

/// Resolve a 1-based (positive) or end-relative (negative) OBJ index against the
/// vertices seen so far. Returns `None` for out-of-range or zero indices.
fn resolve(i: i64, len: usize) -> Option<usize> {
    if i > 0 {
        let idx = (i - 1) as usize;
        (idx < len).then_some(idx)
    } else if i < 0 {
        let idx = len as i64 + i;
        (idx >= 0).then_some(idx as usize)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn parse_str(s: &str) -> MeshStats {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(s.as_bytes()).unwrap();
        parse_path(tmp.path()).unwrap()
    }

    // A 2x3x4 box with quad faces + negative indices in the last two faces.
    const BOX_OBJ: &str = "\
v 0 0 0
v 2 0 0
v 2 3 0
v 0 3 0
v 0 0 4
v 2 0 4
v 2 3 4
v 0 3 4
f 1 4 3 2
f 5 6 7 8
f 1 2 6 5
f 3 4 8 7
f 2 3 7 6
f -8 -5 -1 -4
";

    #[test]
    fn box_geometry_with_quads_and_negative_indices() {
        let stats = parse_str(BOX_OBJ);
        assert_eq!(stats.triangle_count, 12); // 6 quads → 12 tris
        assert_eq!(stats.vertex_count, 8);
        assert_eq!(stats.bbox, [2.0, 3.0, 4.0]);
        assert!((stats.volume - 24.0).abs() < 1e-6, "volume {}", stats.volume);
        assert!((stats.surface_area - 52.0).abs() < 1e-6, "area {}", stats.surface_area);
        assert!(stats.is_watertight);
    }

    #[test]
    fn ignores_normals_uvs_and_comments() {
        let stats = parse_str("# a triangle\nv 0 0 0\nvt 0 0\nvn 0 0 1\nv 1 0 0\nv 0 1 0\nf 1/1/1 2/2/1 3/3/1\n");
        assert_eq!(stats.triangle_count, 1);
        assert_eq!(stats.vertex_count, 3);
        assert!(!stats.is_watertight);
    }
}
