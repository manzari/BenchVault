//! Builds a throwaway demo library for screenshots. Goes through the real
//! service layer rather than shipping a prebuilt .db, so the result is always
//! whatever the current migrations + ingest code actually produce.
//!
//!   seed_demo [<data-dir>] [--force]
//!
//! Data dir defaults to $BENCHVAULT_DATA_DIR, else the platform app-data path.
//! Refuses to touch a dir that already holds a benchvault.db unless --force.

use std::f32::consts::PI;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use benchvault_lib::db;
use benchvault_lib::domain::{CreateObjectInput, EntityKind, FolderId, ObjectId};
use benchvault_lib::library::Library;
use benchvault_lib::repository;
use benchvault_lib::service;

type Tri = [[f32; 3]; 3];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let force = args.iter().any(|a| a == "--force");
    // Deliberately no default: falling back to the real app-data path would put
    // a footgun one stray invocation away from wiping someone's actual library.
    let data_dir = match args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("BENCHVAULT_DATA_DIR").map(PathBuf::from))
    {
        Some(d) => d,
        None => anyhow::bail!(
            "usage: seed_demo <data-dir> [--force]  (or set BENCHVAULT_DATA_DIR)\n\
             Refusing to guess — this wipes whatever library lives at the target."
        ),
    };

    let db_path = data_dir.join("benchvault.db");
    if db_path.exists() {
        if !force {
            anyhow::bail!(
                "{} already has a benchvault.db — refusing to seed. Pass --force to wipe it.",
                data_dir.display()
            );
        }
        // WAL leaves -wal/-shm siblings; a stale pair against a fresh db corrupts reads.
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", db_path.display()));
        }
        let _ = fs::remove_dir_all(data_dir.join("library"));
    }

    fs::create_dir_all(&data_dir)?;
    let library = Library::new(data_dir.clone());
    library.ensure_dirs()?;

    let pool = db::init_pool(&db_path).await?;
    db::run_migrations(&pool).await?;

    let fixtures = data_dir.join(".seed-fixtures");
    fs::create_dir_all(&fixtures)?;

    // --- tags ---
    let tag = |name: &str, color: &str| {
        let pool = pool.clone();
        let (name, color) = (name.to_string(), color.to_string());
        async move { repository::tags::create(&pool, &name, Some(&color)).await }
    };
    let t_pla = tag("PLA", "#4dabf7").await?;
    let t_petg = tag("PETG", "#20c997").await?;
    let t_functional = tag("functional", "#845ef7").await?;
    let t_calibration = tag("calibration", "#fab005").await?;
    let t_printed = tag("printed", "#51cf66").await?;

    // --- folders ---
    let f_upgrades = folder(&pool, &library, None, "Printer Upgrades").await?;
    let f_ender = folder(&pool, &library, Some(f_upgrades), "Ender 3").await?;
    let f_household = folder(&pool, &library, None, "Household").await?;
    let f_calib = folder(&pool, &library, None, "Calibration").await?;

    // --- objects ---
    let cube_stl = write_stl(&fixtures, "xyz_calibration_cube.stl", &cube(20.0))?;
    let cube_obj = object(
        &pool,
        &library,
        "XYZ Calibration Cube",
        Some(f_calib),
        Some("Standard 20mm cube for dialling in dimensional accuracy. Measure each axis with calipers; anything outside ±0.1mm means steps/mm needs a look."),
        Some("https://www.thingiverse.com/thing:1278865"),
        Some("CHEP"),
        Some("CC BY 4.0"),
        &[&cube_stl],
    )
    .await?;
    add_gcode(
        &pool,
        &library,
        cube_obj,
        &write_gcode(
            &fixtures,
            "xyz_cube_0.2_PLA.gcode",
            GcodeSpec {
                slicer: "PrusaSlicer",
                version: "2.8.1",
                time: "38m 12s",
                filament_mm: 1284.4,
                filament_g: 3.83,
                filament_type: "PLA",
                layer_height: 0.2,
                nozzle: 215,
                bed: 60,
                profile: "0.20mm QUALITY @ENDER3",
            },
        )?,
    )
    .await?;
    add_doc(
        &pool,
        &library,
        cube_obj,
        &write_text(
            &fixtures,
            "NOTES.md",
            "# Calibration log\n\n| Date | X | Y | Z | Notes |\n|---|---|---|---|---|\n| 2026-03-02 | 20.04 | 20.06 | 19.98 | Stock steps/mm |\n| 2026-03-09 | 20.01 | 20.02 | 20.00 | After e-steps tune |\n\n## Next\n\n- [x] Recalibrate extruder\n- [ ] Re-run at 0.12mm layer height\n",
        )?,
    )
    .await?;
    attach(&pool, cube_obj, &[t_calibration.id, t_pla.id, t_printed.id]).await?;

    let clip_stl = write_stl(&fixtures, "cable_clip.stl", &torus(9.0, 3.0, 48, 20))?;
    let clip_obj = object(
        &pool,
        &library,
        "Cable Clip",
        Some(f_household),
        Some("Snap-fit clip for routing desk cables along the underside of the tabletop. Printed in PETG so it keeps its spring in a warm room."),
        None,
        None,
        None,
        &[&clip_stl],
    )
    .await?;
    add_gcode(
        &pool,
        &library,
        clip_obj,
        &write_gcode(
            &fixtures,
            "cable_clip_0.2_PETG.gcode",
            GcodeSpec {
                slicer: "OrcaSlicer",
                version: "2.2.0",
                time: "1h 6m 40s",
                filament_mm: 3120.9,
                filament_g: 9.71,
                filament_type: "PETG",
                layer_height: 0.2,
                nozzle: 240,
                bed: 80,
                profile: "0.20mm Standard @BBL X1C",
            },
        )?,
    )
    .await?;
    attach(&pool, clip_obj, &[t_petg.id, t_functional.id, t_printed.id]).await?;

    let knob_stl = write_stl(&fixtures, "bed_level_knob.stl", &hex_prism(14.0, 9.0))?;
    let knob_obj = object(
        &pool,
        &library,
        "Bed Levelling Knob",
        Some(f_ender),
        Some("Oversized knurled knob — much easier to turn than the stock wheel when the bed is hot."),
        None,
        None,
        Some("CC BY-NC 4.0"),
        &[&knob_stl],
    )
    .await?;
    attach(&pool, knob_obj, &[t_pla.id, t_functional.id]).await?;

    let guide_stl = write_stl(&fixtures, "filament_guide.stl", &pyramid(24.0, 30.0))?;
    let guide_obj = object(
        &pool,
        &library,
        "Filament Guide",
        Some(f_upgrades),
        Some("Keeps the filament path off the frame so the extruder stops skipping on long retractions."),
        None,
        None,
        None,
        &[&guide_stl],
    )
    .await?;
    add_gcode(
        &pool,
        &library,
        guide_obj,
        &write_gcode(
            &fixtures,
            "filament_guide_0.3_PLA.gcode",
            GcodeSpec {
                slicer: "SuperSlicer",
                version: "2.5.59",
                time: "22m 5s",
                filament_mm: 742.0,
                filament_g: 2.21,
                filament_type: "PLA",
                layer_height: 0.3,
                nozzle: 210,
                bed: 55,
                profile: "0.30mm DRAFT @ENDER3",
            },
        )?,
    )
    .await?;
    attach(&pool, guide_obj, &[t_pla.id, t_functional.id]).await?;

    // Fixtures were copied into the managed library by the ingest path; the
    // staging copies are just clutter in the demo image.
    let _ = fs::remove_dir_all(&fixtures);

    println!("seeded {} — 4 objects, 4 folders, 5 tags", data_dir.display());
    Ok(())
}

async fn folder(
    pool: &sqlx::SqlitePool,
    library: &Library,
    parent: Option<FolderId>,
    name: &str,
) -> anyhow::Result<FolderId> {
    Ok(service::folders::create(pool, library, parent, name)
        .await
        .map_err(|e| anyhow::anyhow!("folder {name}: {e}"))?
        .id)
}

#[allow(clippy::too_many_arguments)]
async fn object(
    pool: &sqlx::SqlitePool,
    library: &Library,
    name: &str,
    folder_id: Option<FolderId>,
    notes: Option<&str>,
    source_url: Option<&str>,
    author: Option<&str>,
    license: Option<&str>,
    stls: &[&Path],
) -> anyhow::Result<ObjectId> {
    let input = CreateObjectInput {
        name: name.to_string(),
        folder_id,
        notes: notes.map(str::to_string),
        source_url: source_url.map(str::to_string),
        author: author.map(str::to_string),
        license: license.map(str::to_string),
        stl_paths: stls.iter().map(|p| p.display().to_string()).collect(),
        cover_image_path: None,
    };
    Ok(service::objects::create_with_files(pool, library, input)
        .await
        .map_err(|e| anyhow::anyhow!("object {name}: {e}"))?
        .id)
}

async fn add_gcode(
    pool: &sqlx::SqlitePool,
    library: &Library,
    object_id: ObjectId,
    path: &Path,
) -> anyhow::Result<()> {
    service::files::add_gcode(pool, library, object_id, path)
        .await
        .map_err(|e| anyhow::anyhow!("gcode {}: {e}", path.display()))?;
    Ok(())
}

async fn add_doc(
    pool: &sqlx::SqlitePool,
    library: &Library,
    object_id: ObjectId,
    path: &Path,
) -> anyhow::Result<()> {
    service::files::add_doc(pool, library, object_id, path)
        .await
        .map_err(|e| anyhow::anyhow!("doc {}: {e}", path.display()))?;
    Ok(())
}

async fn attach(
    pool: &sqlx::SqlitePool,
    object_id: ObjectId,
    tags: &[benchvault_lib::domain::TagId],
) -> anyhow::Result<()> {
    for t in tags {
        repository::tags::attach(pool, EntityKind::Object.as_str(), object_id.0, *t).await?;
    }
    Ok(())
}

// --- fixture generation -----------------------------------------------------

struct GcodeSpec {
    slicer: &'static str,
    version: &'static str,
    time: &'static str,
    filament_mm: f64,
    filament_g: f64,
    filament_type: &'static str,
    layer_height: f64,
    nozzle: i32,
    bed: i32,
    profile: &'static str,
}

/// Header/footer comments match what `parsers::gcode::prusa_family` scrapes, so
/// the seeded rows carry real print time / filament figures instead of nulls.
fn write_gcode(dir: &Path, name: &str, s: GcodeSpec) -> anyhow::Result<PathBuf> {
    let path = dir.join(name);
    let mut f = fs::File::create(&path)?;
    writeln!(f, "; generated by {} {} on 2026-03-14 at 19:24:11 UTC", s.slicer, s.version)?;
    writeln!(f, ";\n; Demo fixture — not a printable path.\n;")?;
    writeln!(f, "M104 S{}\nM140 S{}\nG28\nG1 Z0.2 F3000", s.nozzle, s.bed)?;
    for layer in 1..=24 {
        writeln!(f, ";LAYER:{layer}")?;
        writeln!(f, "G1 X{:.2} Y{:.2} E{:.4} F1800", 100.0 + layer as f64, 100.0, layer as f64 * 0.9)?;
    }
    writeln!(f, "M104 S0\nM140 S0\nG28 X0 Y0\nM84")?;
    writeln!(f, "; estimated printing time (normal mode) = {}", s.time)?;
    writeln!(f, "; filament used [mm] = {:.1}", s.filament_mm)?;
    writeln!(f, "; filament used [g] = {:.2}", s.filament_g)?;
    writeln!(f, "; filament_type = {}", s.filament_type)?;
    writeln!(f, "; layer_height = {}", s.layer_height)?;
    writeln!(f, "; first_layer_temperature = {}", s.nozzle)?;
    writeln!(f, "; first_layer_bed_temperature = {}", s.bed)?;
    writeln!(f, "; print_settings_id = {}", s.profile)?;
    Ok(path)
}

fn write_text(dir: &Path, name: &str, body: &str) -> anyhow::Result<PathBuf> {
    let path = dir.join(name);
    fs::write(&path, body)?;
    Ok(path)
}

fn write_stl(dir: &Path, name: &str, tris: &[Tri]) -> anyhow::Result<PathBuf> {
    let path = dir.join(name);
    let mut f = fs::File::create(&path)?;
    f.write_all(&[0u8; 80])?;
    f.write_all(&(tris.len() as u32).to_le_bytes())?;
    for t in tris {
        for c in normal(t) {
            f.write_all(&c.to_le_bytes())?;
        }
        for v in t {
            for c in v {
                f.write_all(&c.to_le_bytes())?;
            }
        }
        f.write_all(&0u16.to_le_bytes())?;
    }
    Ok(path)
}

fn normal(t: &Tri) -> [f32; 3] {
    let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len == 0.0 { [0.0, 0.0, 0.0] } else { [n[0] / len, n[1] / len, n[2] / len] }
}

fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [Tri; 2] {
    [[a, b, c], [a, c, d]]
}

fn cube(s: f32) -> Vec<Tri> {
    let (o, e) = (0.0, s);
    let v = |x: f32, y: f32, z: f32| [x, y, z];
    let mut t = Vec::new();
    t.extend(quad(v(o,o,o), v(e,o,o), v(e,e,o), v(o,e,o)));
    t.extend(quad(v(o,o,e), v(o,e,e), v(e,e,e), v(e,o,e)));
    t.extend(quad(v(o,o,o), v(o,e,o), v(o,e,e), v(o,o,e)));
    t.extend(quad(v(e,o,o), v(e,o,e), v(e,e,e), v(e,e,o)));
    t.extend(quad(v(o,o,o), v(o,o,e), v(e,o,e), v(e,o,o)));
    t.extend(quad(v(o,e,o), v(e,e,o), v(e,e,e), v(o,e,e)));
    t
}

fn pyramid(base: f32, height: f32) -> Vec<Tri> {
    let h = base / 2.0;
    let apex = [0.0, 0.0, height];
    let c = [
        [-h, -h, 0.0], [h, -h, 0.0], [h, h, 0.0], [-h, h, 0.0],
    ];
    let mut t = vec![[c[0], c[2], c[1]], [c[0], c[3], c[2]]];
    for i in 0..4 {
        t.push([c[i], c[(i + 1) % 4], apex]);
    }
    t
}

fn hex_prism(radius: f32, height: f32) -> Vec<Tri> {
    let pt = |i: usize, z: f32| {
        let a = PI / 3.0 * i as f32;
        [radius * a.cos(), radius * a.sin(), z]
    };
    let mut t = Vec::new();
    for i in 1..5 {
        t.push([pt(0, 0.0), pt(i + 1, 0.0), pt(i, 0.0)]);
        t.push([pt(0, height), pt(i, height), pt(i + 1, height)]);
    }
    for i in 0..6 {
        let j = (i + 1) % 6;
        t.extend(quad(pt(i, 0.0), pt(j, 0.0), pt(j, height), pt(i, height)));
    }
    t
}

fn torus(major: f32, minor: f32, seg_u: usize, seg_v: usize) -> Vec<Tri> {
    let pt = |i: usize, j: usize| {
        let u = 2.0 * PI * (i % seg_u) as f32 / seg_u as f32;
        let v = 2.0 * PI * (j % seg_v) as f32 / seg_v as f32;
        let r = major + minor * v.cos();
        [r * u.cos(), r * u.sin(), minor * v.sin()]
    };
    let mut t = Vec::new();
    for i in 0..seg_u {
        for j in 0..seg_v {
            t.extend(quad(pt(i, j), pt(i + 1, j), pt(i + 1, j + 1), pt(i, j + 1)));
        }
    }
    t
}
