# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

All commands run from the repo root. Tauri orchestrates both the Rust backend and the Vite/React frontend.

```sh
npm install                   # install JS deps (npm/pnpm/bun all fine)
npm run tauri dev             # dev: launches Vite + builds & runs the Rust app, hot reload on both sides
npm run tauri build           # production bundles (.deb / .rpm / .AppImage on Linux)
npm run dev                   # frontend-only Vite (no Tauri shell — most invoke() calls will fail)
npm run build                 # tsc -b && vite build (frontend only)
```

Rust-side (run from `src-tauri/`):

```sh
cargo check
cargo build
cargo run --bin export_bindings   # regenerate src/lib/bindings.ts from Rust types/commands
cargo test <name>                 # single test by name substring
```

There is no JS test runner or linter wired up yet. There is no CI.

## Architecture

This is a **Tauri 2** desktop app: a Rust binary that hosts a webview rendering a React/Vite frontend. The two halves talk via Tauri's `invoke()` IPC and an event channel.

### The IPC contract is generated, not hand-written

`tauri-specta` walks `#[tauri::command] #[specta::specta]` functions registered in `src-tauri/src/lib.rs::build_specta_builder()` and emits TypeScript signatures to `src/lib/bindings.ts`. Generation runs automatically in debug builds inside `lib.rs::run()`, and on demand via `cargo run --bin export_bindings`. **Never hand-edit `bindings.ts`.** When you add or change a command, also add it to the `collect_commands![...]` list in `lib.rs` — otherwise it won't be exposed to the frontend and the types won't refresh.

`src/lib/api.ts` is a thin hand-written wrapper around `invoke()` that re-types each command for ergonomics; `src/lib/types.ts` re-exports types from `bindings.ts`. When you rename a command or change a parameter shape, update all three.

### Backend layering (`src-tauri/src/`)

The Rust code follows a strict `commands → service → repository → db` flow. Skipping layers (e.g. SQL inside a command) is wrong.

- `commands/` — Tauri command handlers. Thin: validate inputs, call a service, map `ServiceError` → `String`. Do not put business logic here.
- `service/` — orchestrates filesystem + DB work (e.g. `add_gcode` parses the file, copies it into the library, writes asset + metadata + object_assets rows, touches the parent object). Returns `ServiceResult<T>`.
- `repository/` — every SQL query lives here, one module per table family.
- `parsers/` — pure functions that turn a file on disk into a `ParsedGcode` / STL metadata struct. The gcode parser sniffs the first/last 64 KiB and dispatches to a slicer-specific module (`prusa_family` for PrusaSlicer/SuperSlicer/OrcaSlicer/BambuStudio, `cura`).
- `domain/` — serde + specta types. `ids.rs` declares newtype IDs (`FolderId`, `ObjectId`, `AssetId`, `TagId`); use them through service/repository signatures rather than raw `i64`. `kinds.rs` defines the polymorphic `EntityKind` / `AssetKind` / `ObjectAssetRole` enums. `domain/mod.rs` re-exports everything, so `use crate::domain::Foo` works.
- `library.rs` — owns the on-disk file layout (`<app_data>/library/assets/<yyyy>/<mm>/<asset_id>__<sanitized-name>`). Filenames are sanitized aggressively (`a-zA-Z0-9._-` only).
- `db.rs` — SQLx pool factory (WAL mode, foreign keys on) and `sqlx::migrate!("./migrations")` runner. Migrations run automatically on app start.
- `events.rs` — typed `AppEvent` bus emitted as `"app-event"` to the frontend; reserved for background jobs (`jobs/` is currently a placeholder).
- `AppState` (in `lib.rs`) holds `db: SqlitePool` and `library: Arc<Library>`; commands receive it via `State<'_, AppState>`.

### Schema is intentionally polymorphic

The schema is designed so future entity types (printers, filaments, print jobs, slicer profiles, costs, CAD sources, …) drop in without re-architecting. Treat this as a hard constraint, not a stylistic preference.

- **`assets`** is generic — any kind of file (STL, gcode, image, CAD, other) goes here. Per-kind structured fields live in sidecar tables keyed by `asset_id` (`gcode_metadata`, `stl_metadata`). Don't add narrow columns to `assets`.
- **`tags` + `entity_tags`** — `entity_tags(entity_kind, entity_id, tag_id)` lets any entity type be tagged.
- **`entity_links(from_kind, from_id, to_kind, to_id, relation)`** — generic graph between entities for any future relationships.
- **`search_index`** is a single FTS5 virtual table keyed by `(entity_kind, entity_id)`. New entities get indexed by adding insert/update/delete triggers in a new migration; the schema doesn't change.
- v1 entities (`folders`, `objects`, `object_assets`) are concrete tables, but they coexist with — and reference — the polymorphic primitives.

When adding a new entity type:
1. New migration file under `src-tauri/migrations/` (numbered, `NNNN_name.sql`). Never edit existing migration files — append new ones.
2. New `EntityKind` variant in `domain/kinds.rs`.
3. New `*_ai/au/ad` triggers in the migration to keep `search_index` populated.
4. New domain struct, repository module, service functions, commands. Wire commands into `collect_commands![...]` in `lib.rs`.

### Frontend (`src/`)

- TanStack Router: routes are declared imperatively in `main.tsx` (`/`, `/objects/$objectId`, `/search`).
- Mantine 7 for UI; theme set in `main.tsx`.
- Zustand stores in `src/lib/stores/` (`library`, `ui`).
- `@react-three/fiber` + `three` for STL preview (`components/StlViewer.tsx`); STL bytes are pulled across IPC via `read_asset_bytes`.
- File-picker flows use `@tauri-apps/plugin-dialog`; the user picks a path, then commands like `add_stl(object_id, source_path)` ingest it (the Rust side copies into the managed library).

## Long-term scope

This is a 3D **project** database, not just a 3D-printing file manager