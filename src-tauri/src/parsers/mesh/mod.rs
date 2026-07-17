pub mod stl;

/// Geometry computed from a triangle mesh. Distances are in the file's own units
/// (millimeters for STL). Shared across mesh formats so OBJ/3MF can populate the
/// same `mesh_metadata` row later.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshStats {
    pub triangle_count: i64,
    pub vertex_count: i64,
    pub bbox: [f64; 3],
    pub volume: f64,
    pub surface_area: f64,
    pub is_watertight: bool,
}

/// Accumulates triangles into a [`MeshStats`]: bounding box, signed volume,
/// surface area, welded vertex count, and edge-manifoldness (watertight).
///
/// Vertices are welded on a micron grid so shared edges between adjacent
/// triangles are recognized despite float noise. A mesh is watertight when every
/// undirected edge is shared by exactly two triangles.
#[derive(Default)]
pub struct MeshAccumulator {
    tris: i64,
    min: [f64; 3],
    max: [f64; 3],
    signed_vol: f64,
    area: f64,
    verts: std::collections::HashMap<[i64; 3], u32>,
    edges: std::collections::HashMap<(u32, u32), i32>,
    degenerate_edge: bool,
    seen_any: bool,
}

const WELD_SCALE: f64 = 1000.0; // micron grid

impl MeshAccumulator {
    fn key(v: [f64; 3]) -> [i64; 3] {
        [
            (v[0] * WELD_SCALE).round() as i64,
            (v[1] * WELD_SCALE).round() as i64,
            (v[2] * WELD_SCALE).round() as i64,
        ]
    }

    fn intern(&mut self, v: [f64; 3]) -> u32 {
        let next = self.verts.len() as u32;
        *self.verts.entry(Self::key(v)).or_insert(next)
    }

    fn edge(&mut self, a: u32, b: u32) {
        if a == b {
            self.degenerate_edge = true;
            return;
        }
        let key = if a < b { (a, b) } else { (b, a) };
        *self.edges.entry(key).or_insert(0) += 1;
    }

    pub fn add_triangle(&mut self, v0: [f64; 3], v1: [f64; 3], v2: [f64; 3]) {
        if !self.seen_any {
            self.min = v0;
            self.max = v0;
            self.seen_any = true;
        }
        for v in [v0, v1, v2] {
            for i in 0..3 {
                self.min[i] = self.min[i].min(v[i]);
                self.max[i] = self.max[i].max(v[i]);
            }
        }
        self.tris += 1;
        self.signed_vol += dot(v0, cross(v1, v2)) / 6.0;
        let e1 = sub(v1, v0);
        let e2 = sub(v2, v0);
        self.area += 0.5 * norm(cross(e1, e2));

        let a = self.intern(v0);
        let b = self.intern(v1);
        let c = self.intern(v2);
        self.edge(a, b);
        self.edge(b, c);
        self.edge(c, a);
    }

    pub fn finish(self) -> MeshStats {
        let bbox = if self.seen_any {
            [
                self.max[0] - self.min[0],
                self.max[1] - self.min[1],
                self.max[2] - self.min[2],
            ]
        } else {
            [0.0; 3]
        };
        let is_watertight = self.tris > 0
            && !self.degenerate_edge
            && self.edges.values().all(|&c| c == 2);
        MeshStats {
            triangle_count: self.tris,
            vertex_count: self.verts.len() as i64,
            bbox,
            volume: self.signed_vol.abs(),
            surface_area: self.area,
            is_watertight,
        }
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
