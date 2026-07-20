# Phase 4 — 3MF metadata extraction

Status: **planned, not started.** Phases 1–3 (STL, OBJ, SCAD) are complete and
committed. This plan continues the same pattern: parse a file into the shared
`mesh_metadata` sidecar (+ `assets.metadata_json` for loose data), wire all three
ingest paths (picker / watcher / backfill), then surface it in the UI.

3MF is the richest of the target formats and the **first one that needs new
crates**. It is a zip (OPC) container of XML, so unlike STL/OBJ we can't just scan
bytes — we unzip, then parse XML.

---

## 1. What 3MF is

A `.3mf` is an **OPC package** = a ZIP with a fixed part layout:

| Part | Purpose |
|---|---|
| `[Content_Types].xml` | MIME map for parts |
| `_rels/.rels` | Package root relationships → points at the model + thumbnail |
| `3D/3dmodel.model` | The core: XML with `<model unit>`, `<metadata>`, `<resources><object><mesh>`, `<build><item>` |
| `Metadata/thumbnail.png` (path varies; found via a relationship) | Embedded preview image |
| `Metadata/Slic3r_PE.config`, `Metadata/project_settings.config`, `Metadata/Slic3r_PE_model.config`, … | Slicer project settings (PrusaSlicer / SuperSlicer / BambuStudio / OrcaSlicer) — present only in slicer-exported files |

Core model XML shape (namespaced, default ns `http://schemas.microsoft.com/3dmanufacturing/core/2015/02`):

```xml
<model unit="millimeter" xml:lang="en-US">
  <metadata name="Title">Widget</metadata>
  <metadata name="Designer">...</metadata>
  <metadata name="Description">...</metadata>
  <metadata name="LicenseTerms">...</metadata>
  <metadata name="Application">PrusaSlicer-2.7</metadata>
  <resources>
    <object id="1" type="model" name="body">
      <mesh>
        <vertices><vertex x="0" y="0" z="0"/> ...</vertices>
        <triangles><triangle v1="0" v2="1" v3="2"/> ...</triangles>
      </mesh>
    </object>
    <!-- objects may also be assemblies of <components> referencing other objects -->
  </resources>
  <build>
    <item objectid="1" transform="1 0 0 0 1 0 0 0 1 0 0 0"/>
  </build>
</model>
```

Notes that matter:
- `unit` ∈ {`micron`,`millimeter`,`centimeter`,`meter`,`inch`,`foot`}. **This is where `mesh_metadata.source_units` (currently always NULL) finally gets used.** Convert all geometry to mm.
- `<build><item transform="m00 m01 ... ">` is a **row-major 4×3 affine** (12 floats). Geometry that ends up "in the box" is the object mesh(es) referenced by build items, each under its item transform (and nested `<component transform>` for assemblies).
- A file can hold multiple build items / objects → the object's real geometry is the union.

---

## 2. What to extract

| Data | Destination | Notes |
|---|---|---|
| Geometry: triangle/vertex count, bbox, volume, surface area, watertight | `mesh_metadata` (reuse `MeshAccumulator`) | dims/volume/etc. — same as STL/OBJ |
| Units | `mesh_metadata.source_units` | first real use of this column |
| Title / Designer / Description / LicenseTerms / Application / object names | `assets.metadata_json` | loose/variable → JSON, per CLAUDE.md |
| Embedded thumbnail PNG | object cover (via `set_cover_from_png`) | only if the object has no cover yet |
| Slicer project settings | `assets.metadata_json` (or reuse the `gcode_metadata` shape) | present only in slicer exports |

`source_format = "threemf"` on the `mesh_metadata` row (parallels `"stl"`/`"obj"`/`"scad"`).

---

## 3. Dependencies (new)

Add to `src-tauri/Cargo.toml`:
- **`zip`** — read the OPC container. Pure Rust, no system deps.
- **`quick-xml`** — streaming XML reader. Preferred over `roxmltree` because 3MF meshes can be large (hundreds of MB of `<vertex>`/`<triangle>`); streaming avoids loading the whole DOM. Metadata elements are cheap to pull in the same pass.

Both are pure-Rust cargo deps (no host/system libraries), consistent with the "don't bloat the host" rule. Run the build in Docker as usual.

---

## 4. Backend changes (by layer)

### 4.1 Domain — new `AssetKind::ThreeMf`
`src-tauri/src/domain/kinds.rs`:
- Add variant `ThreeMf` to `AssetKind`.
- `as_str` → `"threemf"`; `from_str` `"threemf" => AssetKind::ThreeMf`.
- Serde: add `#[serde(rename = "threemf")]` on the variant (snake_case would emit `three_mf`; mirror the `CadQuery` → `"cadquery"` precedent so wire == DB == `as_str`).
- `assets.kind` is free text (no CHECK constraint), so **no migration** is needed for the new kind value.

### 4.2 Classification — route `.3mf` to its own kind, consistently
Today `.3mf` is handled **inconsistently**, fix this as part of Phase 4:
- `add_attachment` dispatcher (`service/files.rs` ~L323): `.3mf` → `add_stl` → `classify_model_kind` → `AssetKind::Cad`, role `StlPart` (lands in Models, no geometry).
- Watcher `classify` (`jobs/watcher.rs` ~L450): `.3mf` → `(Cad, Attachment)` (lands in **Attachments**, not Models). ← inconsistent with the picker.
- `classify_attachment` (`service/files.rs` ~L872): `.3mf` → `Cad`.

Changes:
- `classify_model_kind` (~L843): `Some("3mf") => AssetKind::ThreeMf`.
- Watcher `classify` (~L450): pull `"3mf"` out of the CAD/attachment arm into `Some("3mf") => Some((AssetKind::ThreeMf, ObjectAssetRole::StlPart))` so dropped 3MF files land in Models like the picker. Route it through `attach_existing_model` (already handles `Stl | Obj | Step`; add `ThreeMf`).
- `mime_for_path` already returns `"model/3mf"` — leave it.

### 4.3 Parser — `parsers/threemf.rs` (new)
Pure function `parse_path(&Path) -> io::Result<ThreeMfParsed>` where:
```rust
struct ThreeMfParsed {
    stats: MeshStats,            // reuse parsers::mesh::MeshAccumulator
    units: Option<String>,       // "millimeter", ...
    metadata: BTreeMap<String,String>, // Title/Designer/Description/LicenseTerms/Application
    object_names: Vec<String>,
    thumbnail: Option<Vec<u8>>,  // PNG bytes if present
    slicer_config: Option<String>, // raw config part text, if any
}
```
Implementation:
1. `zip::ZipArchive` over the file.
2. Read `3D/3dmodel.model` (or follow `_rels/.rels` to the model part — start with the conventional path, fall back to the rels lookup).
3. Stream with `quick-xml`: collect `<metadata name=..>` text, `<object name=..>` names, and feed `<vertex>`/`<triangle>` into `MeshAccumulator`, resolving `<triangle v1 v2 v3>` against the current object's vertex list. Apply the unit scale factor to vertices before accumulating (so bbox/volume/area come out in mm). For a **first cut**, aggregate every object's mesh and apply `<build><item transform>` (identity is the common single-object case); note assemblies/`<component>` transforms as a follow-up if not handled initially.
4. Thumbnail: find the thumbnail relationship (or a `*/thumbnail*.png` part) and read its bytes.
5. Slicer config: read `Metadata/*.config` parts if present (raw text).

Add `MeshStats::to_metadata(asset_id, "threemf")` (helper already exists from Phase 3) — but **also set `source_units`** for 3MF. Either extend `to_metadata` with an optional units arg, or set the field on the returned struct at the call site. Prefer the latter to keep `to_metadata` unchanged for STL/OBJ/SCAD.

Register in `parsers/mod.rs`: `pub mod threemf;`.

### 4.4 Service — wiring
`service/files.rs`:
- `store_mesh_metadata` dispatch (currently `Stl`/`Obj`): add a `ThreeMf` arm. 3MF differs (units + metadata_json + thumbnail), so likely a dedicated `store_threemf(pool, library, object_id, asset_id, path)` that: parses once, upserts `mesh_metadata` (with units), writes metadata_json, and sets the cover-if-missing from the thumbnail. Keep it best-effort/non-fatal like the others.
- `add_stl` already attaches models as `StlPart` — with `classify_model_kind` returning `ThreeMf`, picker 3MF flows correctly; just ensure the mesh/metadata store is invoked for `ThreeMf`.
- `attach_existing_model` — add `ThreeMf` handling (watcher path).
- Backfill: add `"threemf"` to `repository::mesh_metadata::mesh_assets_missing` (kind IN list) so existing 3MF files get geometry on next startup. Metadata_json/thumbnail backfill can piggyback on the same pass or a dedicated `backfill_threemf`.

`repository/mesh_metadata.rs`:
- Extend `mesh_assets_missing` `kind IN ('stl','obj','threemf')`.

### 4.5 Thumbnail → cover
Reuse `set_cover_from_png(pool, library, object_id, png_bytes, name)` (Phase-1 vintage, marks `RENDER_COVER_MARKER`). For 3MF, only set the cover when the object has none, to avoid overriding a user-chosen cover. (Consider a distinct marker like `"3mf_thumbnail"` if we want to distinguish auto-covers later.)

### 4.6 No migrations
Geometry → existing `mesh_metadata` (incl. the unused `source_units` column). Loose metadata → existing `assets.metadata_json`. New kind value needs no schema change. **Phase 4 adds no migration file** unless we decide slicer settings deserve their own sidecar (they don't — `metadata_json` is the right home first).

---

## 5. Frontend changes

- **Geometry shows for free.** With `.3mf` → `AssetKind::ThreeMf` + role `StlPart`, it renders in `StlPartsList` (Models), which already displays dims/volume/△tris/watertight from `mesh_metadata`. No new UI needed for geometry — unlike SCAD (which lives in a separate section).
- **Regenerate bindings.** Adding the `AssetKind::ThreeMf` variant changes the generated union → run `cargo run --bin export_bindings`; update `src/lib/api.ts`/`types.ts` if any switch on kind is exhaustive.
- **In-app 3D preview:** the three.js viewer path only renders STL/OBJ bytes. 3MF won't preview in-app initially (gate it out of `previewableStls`). A later enhancement could parse 3MF → STL bytes for the viewer.
- **Document metadata (4b):** surface Title/Designer/Description/LicenseTerms in the object view — either a small read-only panel on the model row (mirror `ScadList`'s expandable detail) or pre-fill the object sidebar (Author/License/Source) when those are empty. Design decision to make when starting 4b.
- **Units:** optionally show `source_units` next to dimensions when it isn't `millimeter`.

---

## 6. Suggested sub-phasing (each independently shippable)

- **4a — geometry + units + kind.** New `AssetKind::ThreeMf`, consistent routing (fix picker/watcher split), `parsers/threemf.rs` for geometry + units into `mesh_metadata`, backfill, bindings regen. Payoff: 3MF joins the Models list with dims/volume, no new UI. Deps: `zip` + `quick-xml`.
- **4b — document metadata + thumbnail.** Title/Designer/Description/License/Application/object names → `metadata_json`; embedded PNG → cover-if-missing; minimal UI to display the metadata.
- **4c — slicer project settings.** Parse `Metadata/*.config` → `metadata_json` (or a `gcode_metadata`-shaped view since it's the same PrusaSlicer/Bambu key/value world). UI to show print settings alongside gcode.

---

## 7. Testing

- Build tiny 3MF fixtures **in-test** using the `zip` write API (no binary fixtures checked in): a minimal package with `[Content_Types].xml`, `_rels/.rels`, and a `3dmodel.model` containing a unit-cube box.
- Assert: box geometry matches the STL/OBJ box tests (12 tris, 8 verts, bbox, volume 24, area 52, watertight) — confirms the shared accumulator; **unit conversion** (e.g. `unit="centimeter"` → bbox ×10, volume ×1000); metadata map extraction; thumbnail bytes returned when present; graceful handling of a slicer-config part.
- Keep parser tests pure (no DB), matching `parsers/mesh/*` style.

---

## 8. Risks / open questions

- **Assemblies / components / nested transforms.** First cut may aggregate flat meshes + build-item transforms and defer nested `<component>` composition. Note the limitation in code if not handled — silent under-counting of multi-part 3MFs is the failure mode.
- **Namespaces & extensions.** 3MF has extension namespaces (materials, beam lattice, etc.). We only need core geometry + metadata; ignore unknown elements.
- **Model part path.** Conventionally `3D/3dmodel.model` but technically discovered via `_rels/.rels`. Start conventional, fall back to rels.
- **Large files.** Streaming (`quick-xml`) keeps memory bounded; don't collect all vertices into a DOM.
- **Watertight semantics.** 3MF spec requires manifold meshes, but exported files aren't always clean — the existing edge-manifold test is the right check; expect some `is_watertight = false`.
