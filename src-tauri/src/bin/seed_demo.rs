//! Builds a throwaway demo library for screenshots. Goes through the real
//! service layer rather than shipping a prebuilt .db, so the result is always
//! whatever the current migrations + ingest code actually produce.
//!
//!   THINGIVERSE_TOKEN=... seed_demo <data-dir> [--force]
//!
//! Objects are imported from real Thingiverse things through the same
//! `service::imports` path the app's "Import from URL" button uses, so the demo
//! library holds genuine models, covers, authors and licenses. That needs a
//! Thingiverse app token in $THINGIVERSE_TOKEN (https://www.thingiverse.com/developers/apps).
//!
//! Data dir defaults to $BENCHVAULT_DATA_DIR.
//! Refuses to touch a dir that already holds a benchvault.db unless --force.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use benchvault_lib::db;
use benchvault_lib::domain::{
    CreateObjectInput, EntityKind, FolderId, ObjectId, UpdateObjectPatch,
};
use benchvault_lib::library::Library;
use benchvault_lib::repository;
use benchvault_lib::service;

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

    // Every object comes from Thingiverse, so without a token there is nothing
    // to seed — fail up front rather than producing an empty-looking library.
    let token = std::env::var("THINGIVERSE_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "THINGIVERSE_TOKEN is not set.\n\
                 The demo library is built by importing real things from Thingiverse, which needs\n\
                 an app token: create an app at https://www.thingiverse.com/developers/apps and\n\
                 copy its App Token into the repo-root .env as THINGIVERSE_TOKEN=..."
            )
        })?;
    service::imports::set_thingiverse_token(&pool, Some(&token)).await?;

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
    let t_gridfinity = tag("gridfinity", "#e64980").await?;
    let t_repair = tag("repair", "#fd7e14").await?;

    // --- folders ---
    let f_calib = folder(&pool, &library, None, "Calibration").await?;
    let f_workshop = folder(&pool, &library, None, "Workshop").await?;
    let f_gridfinity = folder(&pool, &library, Some(f_workshop), "Gridfinity").await?;
    let f_holders = folder(&pool, &library, Some(f_workshop), "Tool Holders").await?;
    let f_jigs = folder(&pool, &library, Some(f_workshop), "Jigs & Fixtures").await?;
    let f_household = folder(&pool, &library, None, "Household").await?;
    let f_kitchen = folder(&pool, &library, Some(f_household), "Kitchen").await?;
    let f_bath = folder(&pool, &library, Some(f_household), "Bathroom").await?;
    let f_desk = folder(&pool, &library, Some(f_household), "Desk").await?;
    let f_repairs = folder(&pool, &library, None, "Repairs").await?;
    let f_caravan = folder(&pool, &library, Some(f_repairs), "Caravan").await?;
    let f_appliances = folder(&pool, &library, Some(f_repairs), "Appliances").await?;
    let f_upgrades = folder(&pool, &library, None, "Printer Upgrades").await?;
    let f_ender = folder(&pool, &library, Some(f_upgrades), "Ender 3").await?;
    let f_mk3 = folder(&pool, &library, Some(f_upgrades), "Prusa MK3S").await?;

    // --- filler objects: no files, they exist to give the folder tree depth ---
    let stubs: &[(FolderId, &str, &str, &[benchvault_lib::domain::TagId])] = &[
        (f_calib, "Temperature Tower", "220→190°C in 10° steps.", &[t_pla.id]),
        (f_calib, "Retraction Test", "Twin towers for stringing.", &[t_pla.id]),
        (f_calib, "Flow Cube", "Single-wall cube for extrusion multiplier.", &[]),
        (f_calib, "First Layer Patch", "Squish check before long prints.", &[t_printed.id]),
        (f_calib, "Bridging Test", "Unsupported spans up to 60mm.", &[]),
        (f_calib, "Overhang Fan", "45–80° wedges for part-cooling tuning.", &[]),
        (f_gridfinity, "Baseplate 4x4", "Screwed to the bench under the vice.", &[t_gridfinity.id, t_printed.id]),
        (f_gridfinity, "Caliper Bin 1x3", "Cutout for the digital calipers.", &[t_gridfinity.id]),
        (f_gridfinity, "Screwdriver Bin 2x2", "Angled slots, handles facing out.", &[t_gridfinity.id]),
        (f_gridfinity, "Label Holder", "Clip-on for the front lip of a bin.", &[t_gridfinity.id]),
        (f_holders, "Soldering Iron Stand", "Weighted base, brass wool cup.", &[t_functional.id, t_printed.id]),
        (f_holders, "Drill Bit Index", "1–10mm in 0.5 steps.", &[t_functional.id]),
        (f_holders, "Hex Key Rack", "Wall mount, metric set.", &[t_petg.id]),
        (f_holders, "Bench Hook", "Clamps to the front edge.", &[]),
        (f_jigs, "Corner Clamp 90°", "For glue-ups on plywood boxes.", &[t_petg.id, t_functional.id]),
        (f_jigs, "Drilling Guide 6mm", "Steel bushing pressed in.", &[t_functional.id]),
        (f_jigs, "Sanding Block", "Takes a quarter sheet.", &[t_printed.id]),
        (f_kitchen, "Egg Cup", "Prints without supports at 45°.", &[t_pla.id, t_printed.id]),
        (f_kitchen, "Spice Jar Lid", "Fits the 63mm supermarket jars.", &[t_pla.id]),
        (f_kitchen, "Herb Stripper", "Graduated holes for thyme and rosemary.", &[t_petg.id]),
        (f_kitchen, "Coffee Scoop 10g", "Levelled, matched to the grinder.", &[t_printed.id]),
        (f_bath, "Toothbrush Holder", "Wall mount, drains into the sink.", &[t_petg.id]),
        (f_bath, "Razor Hook", "Adhesive pad, no drilling in the tiles.", &[]),
        (f_desk, "Cable Clip", "Snap-fit, routes along the desk underside.", &[t_petg.id, t_functional.id, t_printed.id]),
        (f_desk, "Headphone Hook", "Clamps to the monitor arm.", &[t_printed.id]),
        (f_desk, "Monitor Riser Foot", "Pair of them, 60mm lift.", &[t_pla.id]),
        (f_desk, "Pen Cup", "Hex pattern, prints vase mode.", &[]),
        (f_caravan, "Blind Pull Cord Knob", "The originals go brittle in the sun.", &[t_repair.id, t_petg.id]),
        (f_caravan, "Window Catch", "Left-hand version; mirror for the right.", &[t_repair.id]),
        (f_caravan, "Table Leg Foot", "Stops the leg chewing the floor.", &[t_repair.id, t_printed.id]),
        (f_appliances, "Dishwasher Rack Cap", "Rust guard for the bent tines.", &[t_repair.id]),
        (f_appliances, "Vacuum Hose Adapter", "35mm hose to the dust extractor port.", &[t_repair.id, t_petg.id]),
        (f_appliances, "Washing Machine Filter Cap", "Spare, the tab snapped off.", &[t_repair.id]),
        (f_ender, "Filament Guide", "Keeps the path off the frame.", &[t_pla.id, t_functional.id, t_printed.id]),
        (f_ender, "Bed Levelling Knob", "Oversized, easier on a hot bed.", &[t_pla.id, t_functional.id]),
        (f_ender, "Belt Tensioner", "Y axis; replaces the zip tie.", &[t_petg.id]),
        (f_ender, "Display Cover", "Keeps dust out of the LCD.", &[]),
        (f_mk3, "Spool Holder Arm", "Bearing-mounted, for 2kg spools.", &[t_functional.id]),
        (f_mk3, "PSU Cover Vent", "Louvred, drops the PSU by ~6°C.", &[t_petg.id, t_printed.id]),
    ];
    for (folder_id, name, notes, tags) in stubs {
        let id = stub(&pool, &library, *folder_id, name, notes).await?;
        attach(&pool, id, tags).await?;
    }

    // --- objects (imported from Thingiverse through the real import path) ---
    let benchy = import(
        &pool,
        &library,
        763622,
        Some(f_calib),
        "The reference torture test. Every new filament or nozzle change gets one before anything \
         that matters goes on the bed — the bow overhang and the chimney hole tell you most of \
         what you need to know.",
    )
    .await?;
    add_gcode(
        &pool,
        &library,
        benchy,
        &write_gcode(
            &fixtures,
            "3DBenchy_0.2_PLA.gcode",
            GcodeSpec {
                slicer: "PrusaSlicer",
                version: "2.8.1",
                time: "1h 42m 30s",
                filament_mm: 3540.2,
                filament_g: 10.56,
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
        benchy,
        &write_text(
            &fixtures,
            "NOTES.md",
            "# Benchy log\n\n| Date | Filament | Result |\n|---|---|---|\n| 2026-03-02 | Prusament PLA Galaxy Black | Stringing across the cabin windows |\n| 2026-03-09 | Prusament PLA Galaxy Black | Clean after +0.4mm retraction |\n\n## Next\n\n- [x] Tune retraction\n- [ ] Re-run at 0.12mm layer height\n",
        )?,
    )
    .await?;
    attach(&pool, benchy, &[t_calibration.id, t_pla.id, t_printed.id]).await?;

    let hex_holder = import(
        &pool,
        &library,
        7379183,
        Some(f_gridfinity),
        "Gridfinity bin for hex bits, held by an integrated compliant spring instead of a friction \
         fit — the bits stay put upside down but still pull out by hand. Print with random seams \
         so they don't line up along one of the thin springs.",
    )
    .await?;
    add_gcode(
        &pool,
        &library,
        hex_holder,
        &write_gcode(
            &fixtures,
            "hex_bit_holder_2x3_0.2_PETG.gcode",
            GcodeSpec {
                slicer: "OrcaSlicer",
                version: "2.2.0",
                time: "3h 18m 04s",
                filament_mm: 9840.7,
                filament_g: 30.62,
                filament_type: "PETG",
                layer_height: 0.2,
                nozzle: 240,
                bed: 80,
                profile: "0.20mm Standard @BBL X1C",
            },
        )?,
    )
    .await?;
    attach(
        &pool,
        hex_holder,
        &[t_gridfinity.id, t_functional.id, t_petg.id, t_printed.id],
    )
    .await?;

    let threads = import(
        &pool,
        &library,
        5445157,
        Some(f_workshop),
        "Printable metric threads and screws to pull from when a design needs a fastener that \
         doesn't exist in the parts bin.",
    )
    .await?;
    attach(&pool, threads, &[t_functional.id, t_pla.id]).await?;

    let cutter = import(
        &pool,
        &library,
        2938190,
        Some(f_kitchen),
        "Calavera cookie cutter for Día de los Muertos. Printed in PLA and only ever used on cold \
         dough — it goes nowhere near the dishwasher.",
    )
    .await?;
    add_gcode(
        &pool,
        &library,
        cutter,
        &write_gcode(
            &fixtures,
            "skull_cookie_cutter_0.3_PLA.gcode",
            GcodeSpec {
                slicer: "SuperSlicer",
                version: "2.5.59",
                time: "41m 18s",
                filament_mm: 1420.5,
                filament_g: 4.24,
                filament_type: "PLA",
                layer_height: 0.3,
                nozzle: 210,
                bed: 55,
                profile: "0.30mm DRAFT @ENDER3",
            },
        )?,
    )
    .await?;
    attach(&pool, cutter, &[t_pla.id, t_printed.id]).await?;

    let knob = import(
        &pool,
        &library,
        7168368,
        Some(f_repairs),
        "The original control knob on the Truma B10 boiler cracked around the D-shaft. PETG holds \
         up to the heat next to the flue where PLA crept.",
    )
    .await?;
    add_doc(
        &pool,
        &library,
        knob,
        &write_text(
            &fixtures,
            "FITTING.md",
            "# Fitting notes\n\n- Shaft is a 6mm D-cut; the printed bore needs no reaming at 0.4mm nozzle / 3 perimeters.\n- Push on cold — the shaft splines bite into PETG and hold without a grub screw.\n- 100% infill. At 20% the first one split along the D-flat within a season.\n",
        )?,
    )
    .await?;
    attach(
        &pool,
        knob,
        &[t_repair.id, t_petg.id, t_functional.id, t_printed.id],
    )
    .await?;

    // Don't leave a live API token sitting in a database that gets published as
    // screenshots and copied around.
    service::imports::set_thingiverse_token(&pool, None).await?;

    // Fixtures were copied into the managed library by the ingest path; the
    // staging copies are just clutter in the demo image.
    let _ = fs::remove_dir_all(&fixtures);

    println!(
        "seeded {} — {} objects (5 imported), 15 folders, 7 tags",
        data_dir.display(),
        5 + stubs.len()
    );
    Ok(())
}

/// Object with no files — filler so the folder tree has some depth to show.
async fn stub(
    pool: &sqlx::SqlitePool,
    library: &Library,
    folder_id: FolderId,
    name: &str,
    notes: &str,
) -> anyhow::Result<ObjectId> {
    let input = CreateObjectInput {
        name: name.to_string(),
        folder_id: Some(folder_id),
        notes: Some(notes.to_string()),
        source_url: None,
        author: None,
        license: None,
        stl_paths: Vec::new(),
        cover_image_path: None,
    };
    Ok(service::objects::create_with_files(pool, library, input)
        .await
        .map_err(|e| anyhow::anyhow!("object {name}: {e}"))?
        .id)
}

/// Imports a thing through the same service the app's "Import from URL" uses,
/// then layers on the notes (the importer doesn't carry a description over).
async fn import(
    pool: &sqlx::SqlitePool,
    library: &Library,
    thing_id: u64,
    folder_id: Option<FolderId>,
    notes: &str,
) -> anyhow::Result<ObjectId> {
    let url = format!("https://www.thingiverse.com/thing:{thing_id}");
    let report = service::imports::import_from_url(pool, library, &url, folder_id)
        .await
        .map_err(|e| anyhow::anyhow!("import {url}: {e}"))?;
    if let Some(w) = &report.warning {
        eprintln!("  warning: {w}");
    }
    if report.model_files_imported == 0 {
        anyhow::bail!("{url}: no model files downloaded — a demo object with no models is useless");
    }
    println!(
        "  thing:{thing_id} → {} ({} models, {} skipped)",
        report.object.name, report.model_files_imported, report.files_skipped
    );

    let id = report.object.id;
    service::objects::update(
        pool,
        library,
        id,
        UpdateObjectPatch {
            notes: Some(notes.to_string()),
            ..Default::default()
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!("notes for thing:{thing_id}: {e}"))?;
    Ok(id)
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
