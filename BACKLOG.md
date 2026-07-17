# Backlog

Loose, unprioritized ideas. Not commitments — a place to park things before they get scoped.

## MCP server for BenchVault

Expose BenchVault to Claude (and other MCP clients) so an assistant can help manage
the library: query objects/assets/tags, add files, organize folders, kick off "open in"
handoffs, etc.

- The backend is already layered `commands → service → repository → db`, so an MCP
  server should sit as a **new front-end on top of the `service/` layer** — reuse the
  same service functions the Tauri commands call, don't duplicate business logic.
- Likely shape: a separate binary in the `src-tauri` workspace (e.g. `bin/mcp_server`)
  that opens the same SQLite pool + `Library`, speaking MCP over stdio (and/or HTTP/SSE
  for a networked setup).

Tools to investigate:
- **`rmcp`** — the official Rust MCP SDK (`modelcontextprotocol/rust-sdk`). First choice
  since the backend is already Rust; macro-based tool definitions, stdio + SSE transports,
  tokio-native.
- **`rust-mcp-sdk`** / **`mcp-sdk-rs`** — community alternatives if `rmcp` is too heavy
  or missing a transport we need.
- **TypeScript SDK** (`@modelcontextprotocol/sdk`) — fallback only if we'd rather run the
  server in the frontend/node world; means re-exposing DB access outside Rust, so less
  attractive.

Open questions:
- Transport: stdio (local, per-client spawn) vs HTTP/SSE (shared, always-on)?
- Auth / scoping — read-only vs write tools; how much the assistant is allowed to mutate.
- Concurrency with the running Tauri app writing the same SQLite DB (WAL helps, but define
  the ownership story).

## "Open in" handoff for more programs

We already support handing a file off to OrcaSlicer (`service/orca.rs`, command
`open_asset_in_orca`, settings key `tools.orca_path` with `$PATH`/AppImage auto-detection;
currently `.3mf` only). Generalize this so any asset kind can be opened in an appropriate
external program, with a small registry of known apps modeled on the existing Orca path
resolution.

Candidates:
- **Slicers**: PrusaSlicer, Cura, BambuStudio, SuperSlicer (STL/gcode/3mf).
- **CAD**: FreeCAD, Blender, Fusion (STEP/OBJ/CadQuery `.py`).
- **Editors**: system default / VS Code for Markdown and CadQuery scripts.
- **Generic**: "reveal in file manager" + OS "open with default app".

Investigate:
- A configurable app registry (path per program, per-OS discovery of install locations)
  instead of hardcoding the Orca path.
- Mapping `AssetKind` → list of applicable programs so the UI only offers valid targets.
- Per-OS launch (`open` on macOS, `xdg-open`/direct exec on Linux, `start` on Windows).
