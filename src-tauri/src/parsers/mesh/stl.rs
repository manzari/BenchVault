//! STL geometry extraction. Detects binary vs ASCII by the exact-size formula
//! (an 80-byte header + u32 count + 50 bytes/triangle) rather than the "solid"
//! prefix, since binary files often begin with "solid" too.

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek};
use std::path::Path;

use super::{MeshAccumulator, MeshStats};

const BINARY_HEADER: u64 = 84; // 80-byte header + u32 triangle count
const BINARY_TRI: u64 = 50; // 12 f32 + u16 attribute

pub fn parse_path(path: &Path) -> std::io::Result<MeshStats> {
    let len = std::fs::metadata(path)?.len();
    let mut f = BufReader::new(File::open(path)?);
    parse_reader(&mut f, len)
}

/// Parse STL geometry from an in-memory buffer (e.g. the bytes an OpenSCAD render
/// produced) without touching disk.
pub fn parse_bytes(bytes: &[u8]) -> std::io::Result<MeshStats> {
    parse_reader(&mut Cursor::new(bytes), bytes.len() as u64)
}

fn parse_reader<R: Read + Seek>(f: &mut R, len: u64) -> std::io::Result<MeshStats> {
    if is_binary(f, len)? {
        parse_binary(f)
    } else {
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        Ok(parse_ascii(&s))
    }
}

/// Binary when the declared triangle count matches the file size exactly.
fn is_binary<R: Read + Seek>(f: &mut R, len: u64) -> std::io::Result<bool> {
    if len < BINARY_HEADER {
        return Ok(false);
    }
    let mut head = [0u8; 84];
    f.read_exact(&mut head)?;
    f.rewind()?;
    let count = u32::from_le_bytes([head[80], head[81], head[82], head[83]]) as u64;
    Ok(len == BINARY_HEADER + count * BINARY_TRI)
}

fn parse_binary<R: Read>(f: &mut R) -> std::io::Result<MeshStats> {
    let mut header = [0u8; 80];
    f.read_exact(&mut header)?;
    let mut cnt = [0u8; 4];
    f.read_exact(&mut cnt)?;
    let count = u32::from_le_bytes(cnt);

    let mut acc = MeshAccumulator::default();
    let mut tri = [0u8; 50];
    for _ in 0..count {
        if f.read_exact(&mut tri).is_err() {
            break; // truncated file — keep what we parsed
        }
        // Skip the 3-float normal (bytes 0..12); read the three vertices.
        let v0 = read_vertex(&tri, 12);
        let v1 = read_vertex(&tri, 24);
        let v2 = read_vertex(&tri, 36);
        acc.add_triangle(v0, v1, v2);
    }
    Ok(acc.finish())
}

fn read_vertex(tri: &[u8; 50], off: usize) -> [f64; 3] {
    let f = |i: usize| {
        f32::from_le_bytes([tri[i], tri[i + 1], tri[i + 2], tri[i + 3]]) as f64
    };
    [f(off), f(off + 4), f(off + 8)]
}

fn parse_ascii(s: &str) -> MeshStats {
    let mut acc = MeshAccumulator::default();
    let mut verts: Vec<[f64; 3]> = Vec::with_capacity(3);
    for line in s.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("vertex") {
            let mut it = rest.split_whitespace().filter_map(|t| t.parse::<f64>().ok());
            if let (Some(x), Some(y), Some(z)) = (it.next(), it.next(), it.next()) {
                verts.push([x, y, z]);
                if verts.len() == 3 {
                    acc.add_triangle(verts[0], verts[1], verts[2]);
                    verts.clear();
                }
            }
        } else if line.starts_with("endfacet") {
            verts.clear(); // guard against malformed facets
        }
    }
    acc.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    // A unit cube (2x3x4 box) as ASCII STL, 12 triangles, watertight.
    fn box_ascii(sx: f64, sy: f64, sz: f64) -> String {
        let v = [
            [0.0, 0.0, 0.0], [sx, 0.0, 0.0], [sx, sy, 0.0], [0.0, sy, 0.0],
            [0.0, 0.0, sz], [sx, 0.0, sz], [sx, sy, sz], [0.0, sy, sz],
        ];
        // outward-facing triangles (CCW seen from outside)
        let faces = [
            [0, 3, 2], [0, 2, 1], // bottom (z=0), normal -z
            [4, 5, 6], [4, 6, 7], // top (z=sz), normal +z
            [0, 1, 5], [0, 5, 4], // front (y=0)
            [2, 3, 7], [2, 7, 6], // back (y=sy)
            [1, 2, 6], [1, 6, 5], // right (x=sx)
            [0, 4, 7], [0, 7, 3], // left (x=0)
        ];
        let mut s = String::from("solid box\n");
        for face in faces {
            s.push_str("facet normal 0 0 0\nouter loop\n");
            for idx in face {
                let p = v[idx];
                s.push_str(&format!("vertex {} {} {}\n", p[0], p[1], p[2]));
            }
            s.push_str("endloop\nendfacet\n");
        }
        s.push_str("endsolid box\n");
        s
    }

    #[test]
    fn ascii_box_geometry() {
        let stats = parse_ascii(&box_ascii(2.0, 3.0, 4.0));
        assert_eq!(stats.triangle_count, 12);
        assert_eq!(stats.vertex_count, 8);
        assert_eq!(stats.bbox, [2.0, 3.0, 4.0]);
        assert!((stats.volume - 24.0).abs() < 1e-6, "volume {}", stats.volume);
        // surface area = 2*(2*3 + 3*4 + 2*4) = 2*26 = 52
        assert!((stats.surface_area - 52.0).abs() < 1e-6, "area {}", stats.surface_area);
        assert!(stats.is_watertight);
    }

    // Same box, encoded as a binary STL, to exercise parse_bytes + binary detection.
    fn box_binary(sx: f64, sy: f64, sz: f64) -> Vec<u8> {
        let v = [
            [0.0, 0.0, 0.0], [sx, 0.0, 0.0], [sx, sy, 0.0], [0.0, sy, 0.0],
            [0.0, 0.0, sz], [sx, 0.0, sz], [sx, sy, sz], [0.0, sy, sz],
        ];
        let faces = [
            [0, 3, 2], [0, 2, 1], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4],
            [2, 3, 7], [2, 7, 6], [1, 2, 6], [1, 6, 5], [0, 4, 7], [0, 7, 3],
        ];
        let mut b = vec![0u8; 80];
        b.extend_from_slice(&(faces.len() as u32).to_le_bytes());
        for face in faces {
            b.extend_from_slice(&[0u8; 12]); // normal
            for idx in face {
                for c in v[idx] {
                    b.extend_from_slice(&(c as f32).to_le_bytes());
                }
            }
            b.extend_from_slice(&[0u8; 2]); // attribute byte count
        }
        b
    }

    #[test]
    fn binary_box_via_parse_bytes() {
        let stats = parse_bytes(&box_binary(2.0, 3.0, 4.0)).unwrap();
        assert_eq!(stats.triangle_count, 12);
        assert_eq!(stats.vertex_count, 8);
        assert_eq!(stats.bbox, [2.0, 3.0, 4.0]);
        assert!((stats.volume - 24.0).abs() < 1e-6, "volume {}", stats.volume);
        assert!(stats.is_watertight);
    }

    #[test]
    fn open_mesh_is_not_watertight() {
        // Two triangles sharing an edge but leaving a boundary → open.
        let mut acc = MeshAccumulator::default();
        acc.add_triangle([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        acc.add_triangle([1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]);
        let stats = acc.finish();
        assert_eq!(stats.triangle_count, 2);
        assert_eq!(stats.vertex_count, 4);
        assert!(!stats.is_watertight);
    }
}
