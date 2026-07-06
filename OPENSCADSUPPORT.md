# OpenSCAD Support — Implementation Spec

Target: give `.scad` files first-class treatment in BenchVault:

1. **Open with OpenSCAD** — a per-asset action that launches the installed OpenSCAD GUI on the library file (mirroring the existing "Open in OrcaSlicer" flow).
2. **Visual preview** — render the `.scad` to a mesh via the OpenSCAD CLI and display it in the existing three.js STL viewer, with on-disk caching.

This document is a self-contained spec. All file paths, signatures, and CLI flags below were verified against the codebase and OpenSCAD documentation (as of July 2026).

---

## 1. Current state (what exists today)

- `.scad` files are already ingested, but as generic CAD attachments:
  - Watcher: `src-tauri/src/jobs/watcher.rs` `classify()` (~line 425) maps `scad` → `(AssetKind::Cad, ObjectAssetRole::Attachment)`.
  - Manual add: `src-tauri/src/service/files.rs` `classify_attachment()` (~line 632) maps `scad` → `AssetKind::Cad`.
  - MIME: `mime_for_path()` (~line 611) already returns `application/x-openscad` for `scad`.
- The external-tool pattern to copy is `src-tauri/src/service/orca.rs`: binary detection (`detect_orca`), settings key (`tools.orca_path` via `app_settings`), fire-and-forget GUI launch (`spawn_open`), bounded headless run with timeout + kill (`run_bounded`, `BUILD_TIMEOUT`), AppImage/flatpak command-line handling (`build_command`).
- The STL preview pipeline: frontend pulls bytes via `read_asset_bytes` (`src-tauri/src/commands/bytes.rs`), parses with three.js `STLLoader` in `src/components/StlViewer.tsx`, guarded by `stlPreviewMaxBytes` (settings-backed). Cover snapshots go through `src/lib/stlRender.ts` → `api.setCoverFromRender`.
- No `tauri-plugin-shell`/`opener` is installed; external processes are spawned directly with `std::process::Command` (also used by `commands/system.rs::reveal`). Keep that pattern; do not add a plugin.
- Migrations: next free number is **`0007`**.

## 2. Preview strategy — decision

### Chosen: CLI mesh export → existing three.js viewer

```
openscad -o <cache>/<asset_id>_<hash>.stl <library_file>.scad
```

Rationale:

- **Mesh export is truly headless.** STL/3MF/OFF export needs no GPU, no X server, no OpenGL context on any platform or OpenSCAD version.
- **PNG export is not.** `openscad -o out.png` requires an OpenGL context; the stable 2021.01 Linux builds use GLX (needs an X display; Wayland-only or CI setups fail without `xvfb-run`). Only nightlies do offscreen EGL. Also a PNG is a static image — routing through STL gives the user the full interactive orbit/zoom viewer we already have.
- **WASM (openscad-wasm) rejected for v1.** It would remove the "install OpenSCAD" requirement, but: upstream is stale (last release 2022), ~10 MB wasm payload, GPL-2.0 code bundled into the app, fonts and `use <MCAD/...>` imports need manual virtual-FS provisioning. The actively maintained proof of concept (openscad-playground) is not a reusable npm package. Revisit later; the architecture below (a `render_scad_preview` command returning STL bytes) keeps the door open — the frontend won't care where the bytes come from.

### OpenSCAD CLI facts the implementation must respect

- Latest **stable** release is still **2021.01**; everything newer is a dated nightly (e.g. `2025.08.17`). Both must work.
- `--version` prints `OpenSCAD version <ver>` **to stderr**, and old builds returned a non-zero exit code for `--version` — parse output, don't trust the exit code for detection.
- The Manifold backend (`--backend=manifold`, case-insensitive) is 10–100× faster than CGAL but only exists in nightlies; non-experimental since nightly `2024.09.28`, default since `2025.08.17`. Gate the flag: pass it only when the parsed version is a date ≥ `2024.09.28`. Never pass it to `2021.01` (unknown-option error).
- On errors OpenSCAD exits non-zero and writes diagnostics (`ERROR:`, `WARNING:`) **entirely to stderr**. Capture stderr and surface it to the user on failure.
- Windows: the GUI binary is `openscad.exe`, but CLI use must go through **`openscad.com`** (console wrapper) or output is swallowed. For the fire-and-forget GUI open, `openscad.exe` is fine.
- Full render can be slow (complex CGAL models: minutes). The render must be bounded with a kill-on-timeout, exactly like `orca.rs::run_bounded`.
- `-d <deps_file>` writes a make-style dependency file listing every `include`/`use`/`import`ed path — use it for cache invalidation of multi-file projects.

## 3. Domain & schema changes

### 3.1 `AssetKind::Scad` and `ObjectAssetRole::Scad`

`src-tauri/src/domain/kinds.rs`:

- Add `Scad` variant to `AssetKind` → wire string `"scad"` in `as_str()`/`from_str()`.
- Add `Scad` variant to `ObjectAssetRole` → wire string `"scad"`.

This mirrors the Gcode pairing and lets the frontend split the asset list by role, like it already does for `stl_part` and `gcode`.

### 3.2 Migration `src-tauri/migrations/0007_scad.sql`

Data-only migration (no new tables in v1 — no `scad_metadata` sidecar yet; the empty `stl_metadata` precedent shows we shouldn't add one until there's data to put in it):

```sql
-- Reclassify existing .scad assets from cad/attachment to scad/scad
UPDATE assets
SET kind = 'scad', updated_at = strftime('%s','now')
WHERE kind = 'cad' AND lower(original_filename) LIKE '%.scad';

UPDATE object_assets
SET role = 'scad'
WHERE role = 'attachment'
  AND asset_id IN (SELECT id FROM assets WHERE kind = 'scad');
```

No FTS work needed: the existing `assets_ai/au/ad` triggers in `0002_fts.sql` index every asset row by `original_filename` regardless of kind (the `assets_au` trigger fires on the UPDATE above and re-indexes).

### 3.3 Classification updates

- `src-tauri/src/jobs/watcher.rs` `classify()`: move `scad` out of the CAD arm into its own arm → `Some((AssetKind::Scad, ObjectAssetRole::Scad))`.
- `src-tauri/src/service/files.rs` `classify_attachment()`: remove `scad` from the CAD list (a `.scad` picked through the "All files" attachment path should still become a proper scad asset — see `add_scad` below; simplest is to have `add_attachment`'s service fn detect the extension and delegate to `add_scad`, or map it to `AssetKind::Scad` + reattach with role `Scad`).

## 4. Backend: `service/openscad.rs`

New module modeled closely on `service/orca.rs`. Register in `service/mod.rs`.

### 4.1 Binary detection & settings

- Settings key: `tools.openscad_path` in `app_settings` (same repo helpers orca uses).
- `detect_openscad() -> Option<String>`, probed in order:
  - PATH: `openscad`, then `openscad-nightly` (Debian/openSUSE nightly package binary name).
  - Linux: flatpak `org.openscad.OpenSCAD` (probe `flatpak info org.openscad.OpenSCAD`; store as a marker value like orca does for flatpak, and build the command as `flatpak run org.openscad.OpenSCAD <args>`).
  - macOS: `/Applications/OpenSCAD.app/Contents/MacOS/OpenSCAD`.
  - Windows: `C:\Program Files\OpenSCAD\openscad.com` (prefer `.com` for CLI; fall back to `.exe`).
- `resolve_openscad(pool) -> ServiceResult<ResolvedTool>`: settings value if set and still exists, else `detect_openscad()`, else `ServiceError` with the message "OpenSCAD not found — set the path in Settings".
- `openscad_version(cmd) -> Option<String>`: run `--version`, parse `OpenSCAD version (\S+)` from **stderr** (fall back to stdout). Cache per resolved path for the process lifetime (e.g. `OnceLock`/`Mutex<HashMap>`); don't shell out on every render.
- `supports_manifold(version) -> bool`: version parses as `YYYY.MM.DD` and is ≥ `2024.09.28`.

### 4.2 Open in GUI

```rust
pub async fn open_asset(pool: &SqlitePool, library: &Library, asset_id: AssetId) -> ServiceResult<()>
```

- Load asset, verify `kind == AssetKind::Scad`, resolve absolute path via `library.absolute(...)`.
- Resolve binary, `Command::spawn` fire-and-forget (copy `orca.rs::spawn_open`, including the AppImage/flatpak arg handling).
- Note: the flatpak build sandbox has host filesystem access by default on Flathub's OpenSCAD manifest, but if the launch shows a blank editor, that's the first thing to check — mention in the error path is unnecessary, just keep the plain spawn.

### 4.3 Preview render with cache

```rust
pub async fn render_preview(pool: &SqlitePool, library: &Library, asset_id: AssetId) -> ServiceResult<Vec<u8>>
```

Cache layout — **outside** the library root so the filesystem watcher never sees it (watcher watches `library.root()`):

```
<app_data>/cache/scad_previews/<asset_id>.stl
<app_data>/cache/scad_previews/<asset_id>.json    // manifest
```

`AppState` (in `lib.rs`) should gain the cache dir (compute once at setup: `app.path().app_data_dir()?.join("cache/scad_previews")`, `create_dir_all`), or add a `Library::cache_dir()` accessor if `Library` already holds the app-data root — implementer's choice, but do not put the cache under `library/`.

Manifest JSON: `{ "deps": [{ "path": ..., "sha256": ... }, ...], "openscad_version": ..., "rendered_at": ... }` where `deps` includes the main `.scad` file itself.

Flow:

1. Load asset, verify kind, resolve source path.
2. **Cache check**: if `<id>.stl` and `<id>.json` exist and every dep in the manifest still hashes to the recorded sha256 (reuse `sha256_file` from `service/files.rs`), return the cached STL bytes.
3. Resolve binary + version. Build the render command:

   ```
   openscad -o <cache>/<id>.stl.tmp -d <cache>/<id>.deps <abs_source>.scad
   ```

   plus `--backend=manifold` when `supports_manifold`. Output extension must be `.stl` for format inference, so use a temp name like `<id>.tmp.stl` and rename into place on success.
4. Run bounded (copy `orca.rs::run_bounded` semantics: `tokio::task::spawn_blocking`, kill on timeout). Timeout: reuse the 180 s constant pattern (`const RENDER_TIMEOUT: Duration = Duration::from_secs(180)`).
5. On non-zero exit: delete temp output, return `ServiceError` carrying the **tail of stderr** (last ~2000 chars — OpenSCAD error messages are at the end). This is what the user sees, so keep it readable: prefix with "OpenSCAD render failed:".
6. On success: parse the make-style deps file (`target: dep1 dep2 \` continuation lines; entries are space-separated with `\`-escaped spaces — a simple line-join + split is fine, ignore the `target:` prefix), hash each dep, write the manifest, atomically rename the temp STL into place, return the bytes.
7. Concurrency: guard with a per-asset (or single global, simpler) `tokio::sync::Mutex` in `AppState` so double-clicking Preview doesn't spawn two renders — follow the existing `usb_sync_lock: Arc<Mutex<()>>` pattern.

Do **not** emit `JobProgress` events for v1 (OpenSCAD gives no progress output anyway); the command is awaited by the frontend with a spinner. `LibraryChanged` must not fire — the cache is not part of the library.

### 4.4 Ingest: `add_scad`

In `src-tauri/src/service/files.rs`, mirror `add_stl` (~line 18):

```rust
pub async fn add_scad(pool, library, object_id: ObjectId, source: &Path) -> ServiceResult<Asset> {
    let asset = ingest_file(pool, library, object_id, source, AssetKind::Scad, mime_for_path(source)).await?;
    let sort = next_sort_order(pool, object_id, ObjectAssetRole::Scad).await?;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::Scad, sort).await?;
    repository::objects::touch(pool, object_id).await?;
    Ok(asset)
}
```

Also add `attach_existing_scad` next to `attach_existing_stl` (~line 488) for the watcher path, and dispatch to it from the watcher's ingest match on the new classify arm.

`remove_asset` needs one addition: after deleting the library file, best-effort delete the two cache files for that asset id (ignore errors).

## 5. Backend: commands

New file `src-tauri/src/commands/openscad.rs` (or extend `commands/files.rs` for `add_scad` and put tool commands beside the orca ones — follow whichever grouping `commands/` uses for orca; the orca commands live under a `tools` grouping in `collect_commands!`):

```rust
#[tauri::command] #[specta::specta]
pub async fn add_scad(state, object_id: ObjectId, source_path: String) -> Result<Asset, AppError>;

#[tauri::command] #[specta::specta]
pub async fn open_asset_in_openscad(state, asset_id: AssetId) -> Result<(), AppError>;

#[tauri::command] #[specta::specta]
pub async fn render_scad_preview(state, asset_id: AssetId) -> Result<Vec<u8>, AppError>;

#[tauri::command] #[specta::specta]
pub async fn get_openscad_path(state) -> Result<Option<String>, AppError>;  // settings value or detected

#[tauri::command] #[specta::specta]
pub async fn set_openscad_path(state, path: Option<String>) -> Result<(), AppError>;
```

Match the exact shape of `get_orca_path`/`set_orca_path` (check whether `get_orca_path` returns the detected fallback when unset, and do the same).

Wiring checklist (all mandatory, see CLAUDE.md):

1. Add all five to `collect_commands![...]` in `src-tauri/src/lib.rs`.
2. Regenerate bindings: `cargo run --bin export_bindings` from `src-tauri/`.
3. Add wrappers to `src/lib/api.ts` (camelCase invoke args, e.g. `invoke<Asset>("add_scad", { objectId, sourcePath })`; `render_scad_preview` returns `number[]`).
4. Re-export any new types from `src/lib/types.ts` if needed.

## 6. Frontend

### 6.1 `ObjectDetailPage.tsx`

- Add `const scads = data.assets.filter(a => a.role === "scad")` next to the existing `stls`/`gcodes` splits (~line 145).
- Render a new `<ScadList scads={scads} objectId={...} onChanged={reload} />` section between the STL parts list and attachments.
- The `AttachmentStrip` filter (whatever excludes `stl_part`/`gcode` roles today) must also exclude role `scad` so files don't show twice.

### 6.2 New `src/components/ScadList.tsx`

Clone the structure of `GcodeList.tsx` (table + add-button + per-row menu). Rows show filename, size. Actions:

- **Add SCAD** button: `@tauri-apps/plugin-dialog` `open()` with `filters: [{ name: "OpenSCAD", extensions: ["scad"] }]`, `multiple: true` → `api.addScad` per path → `onChanged()`.
- **Preview**: calls `onPreview(asset)` (prop) — see viewer integration below. Disable with tooltip "OpenSCAD not found" if `api.getOpenscadPath()` resolved to null (fetch once on mount).
- **Open in OpenSCAD**: `api.openAssetInOpenscad(asset.id)`; on error, notification with the message (mirrors "Open in OrcaSlicer" in `StlPartsList.tsx`).
- **Remove**: `api.removeAsset` with the same confirm flow the other lists use.

### 6.3 Viewer integration

`StlViewer.tsx` currently loads geometry per selected part via `api.readAssetBytes` → `STLLoader().parse()` and caches in a `Map` keyed by asset id. Generalize minimally:

- Extend the `parts` prop entries (or add a parallel prop) so each entry carries a loader discriminator, e.g. change the internal fetch at ~line 135 to:

  ```ts
  const bytes = part.asset.kind === "scad"
    ? await api.renderScadPreview(part.asset.id)
    : await api.readAssetBytes(part.asset.id);
  ```

  Everything downstream (parse, normals, cache, capture-cover) is unchanged — rendered SCAD output is a plain STL.
- In `ObjectDetailPage.tsx`, include `scads` in the list passed to `<StlViewer parts={[...previewableStls, ...scads]}>` so SCAD entries appear in the viewer's part selector alongside STLs.
- Loading state: SCAD first-load can take up to the 180 s timeout. The viewer already has a loading state; change the copy for scad parts to "Rendering with OpenSCAD…" so users understand the wait. On error, show the `ServiceError` message (contains the OpenSCAD stderr tail) in the overlay area where "Too large to preview" renders today.
- **Size guard**: `stlPreviewMaxBytes` guards on `asset.size_bytes`, which for a `.scad` is the tiny source size — meaningless. Apply the guard *after* render instead: if the returned byte length exceeds `stlPreviewMaxBytes`, skip parsing and show the existing "Too large to preview" overlay.
- "Make cover from render" works for free once geometry is in the viewer (`captureCover` path); also enable the `StlPartsList`-style `renderStlToPng` cover action in `ScadList` by feeding it the rendered bytes.

Do **not** add auto-cover-from-scad in `src/lib/cover.ts` for v1 — an automatic 3-minute CLI render on object creation is a footgun. Cover from SCAD stays a manual action.

### 6.4 Pickers

- `AddObjectDialog.tsx` `pickStls` (~line 47): leave as-is for v1 (its paths flow into `create_object`'s `stl_paths` → `add_stl`, which would mis-ingest a `.scad`). Adding scad support to object creation is future work; note it in the PR description.
- `StlPartsList.tsx` `addStl` "All files" branch (~line 63): if the picked file ends in `.scad`, route to `api.addScad` instead of `api.addAttachment`.

### 6.5 Settings UI

Wherever the OrcaSlicer path setting is edited (search for `orca_path` usage in `src/`), add an identical "OpenSCAD path" field backed by `get_openscad_path`/`set_openscad_path`, with placeholder text showing the auto-detected value.

## 7. Edge cases

| Case | Handling |
|---|---|
| OpenSCAD not installed | `resolve_openscad` error → Preview/Open buttons disabled or show a notification pointing to Settings. Ingest and listing work regardless. |
| Script error in `.scad` | Non-zero exit → error with stderr tail shown in viewer overlay. No cache written. |
| Render timeout (180 s) | Process killed, clear error message ("render timed out after 180 s"). Suggest nightly/Manifold in the message only if version is 2021.01. |
| `include`/`use` of files outside the library | Works if the paths still resolve (deps are hashed by absolute path); if a dep is missing, OpenSCAD itself errors — surface it. Manifest hash check treats a missing dep file as cache-invalid, forcing a re-render that produces the real error. |
| `.scad` edited on disk (watcher `on_modify`) | `update_size_sha` path updates the asset row; cache invalidates naturally via the manifest hash on next preview. No proactive re-render. |
| Huge render output | Post-render `stlPreviewMaxBytes` check (§6.3). The cache file is still written (render cost is sunk); only parsing is skipped. |
| Two concurrent preview requests | Serialized by the render mutex; second caller gets the cache hit. |
| Existing `.scad` assets from before this change | Reclassified by migration 0007; appear in the new SCAD section after upgrade. |

## 8. Verification checklist

1. `cargo check` clean; `cargo run --bin export_bindings` regenerates `src/lib/bindings.ts` (never hand-edit it); `npm run build` clean.
2. Migration: open an existing DB containing a `.scad` attachment → it appears in the SCAD list, not attachments, and FTS still finds it by filename.
3. Ingest: Add SCAD via picker; drop a `.scad` into the object folder on disk → watcher ingests with kind/role `scad` (check `LibraryChanged` refresh).
4. Preview: simple cube file renders < 5 s and orbits in the viewer; second preview is instant (cache); edit the file → preview re-renders; a file with `include <lib.scad>` re-renders when only `lib.scad` changes.
5. Errors: syntactically broken `.scad` shows the OpenSCAD error text; with the binary path set to nonsense, buttons degrade gracefully.
6. Open in OpenSCAD launches the GUI on the library copy (verify title bar path).
7. Cover: "make cover from render" on a SCAD part produces a cover image.
8. Windows note for whoever tests there: CLI must resolve to `openscad.com`.

## 9. Future work (explicitly out of scope for v1)

- `scad_metadata` sidecar table (render duration, dep list, customizer params) — add only when something consumes it.
- Customizer parameter UI: OpenSCAD has no schema export (`--export-customizer` is an open upstream request, issue #4457); would require parsing `/* [Group] */` comment annotations ourselves, then rendering with `-p sets.json -P <name>` or `-D var=val`.
- `stl_paths`-style scad support in `create_object` / `AddObjectDialog`.
- WASM in-app rendering (openscad-wasm) as a no-install fallback — blocked on upstream staleness and GPL-2.0 bundling implications.
- Background render queue with `AppEvent::JobProgress` if 180 s foreground waits prove annoying.
