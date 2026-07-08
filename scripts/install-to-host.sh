#!/usr/bin/env bash
# Install the built BenchVault AppImage into ~/Applications/BenchVault.
# Copies the bundle to BenchVault.latest.AppImage and drops:
#   - RunBenchVault.sh   launcher (works without libfuse2; self-detaches)
#   - BenchVault.desktop launcher (Terminal=false — no console window)
#
# Usage:
#   scripts/install-to-host.sh            # install the already-built AppImage
#   scripts/install-to-host.sh --build    # (re)build the AppImage first, then install
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE_DIR="$REPO_ROOT/src-tauri/target/release/bundle/appimage"
DEST_DIR="$HOME/Applications/BenchVault"
DEST_APPIMAGE="$DEST_DIR/BenchVault.latest.AppImage"
RUNNER="$DEST_DIR/RunBenchVault.sh"
ICON="$DEST_DIR/BenchVault.png"
DESKTOP="$DEST_DIR/BenchVault.desktop"

if [[ "${1:-}" == "--build" ]]; then
  echo ">> Building AppImage..."
  ( cd "$REPO_ROOT" && npm run tauri build -- --bundles appimage )
fi

# Pick the newest AppImage in the bundle dir.
SRC_APPIMAGE="$(ls -t "$BUNDLE_DIR"/*.AppImage 2>/dev/null | head -n1 || true)"
if [[ -z "$SRC_APPIMAGE" ]]; then
  echo "!! No AppImage found in $BUNDLE_DIR" >&2
  echo "   Build one first:  scripts/install-to-host.sh --build" >&2
  exit 1
fi

echo ">> Installing: $SRC_APPIMAGE"
mkdir -p "$DEST_DIR"
cp -f "$SRC_APPIMAGE" "$DEST_APPIMAGE"
chmod +x "$DEST_APPIMAGE"

cat > "$RUNNER" <<'EOF'
#!/usr/bin/env bash
# Launch BenchVault, fully detached from the calling shell/terminal so no
# console window lingers. Uses --appimage-extract-and-run so it works without
# libfuse2 (Ubuntu 26.04 ships FUSE3 only). Runs from wherever it lives.
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
setsid "$HERE/BenchVault.latest.AppImage" --appimage-extract-and-run "$@" \
  </dev/null >/dev/null 2>&1 &
disown 2>/dev/null || true
EOF
chmod +x "$RUNNER"

# Icon for the desktop launcher (best-effort; skip if the source is missing).
if [[ -f "$REPO_ROOT/src-tauri/icons/128x128.png" ]]; then
  cp -f "$REPO_ROOT/src-tauri/icons/128x128.png" "$ICON"
fi

# Desktop launcher — Terminal=false is what actually suppresses the console.
cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=BenchVault
Comment=3D project database
Exec=$RUNNER
Icon=$ICON
Terminal=false
Categories=Graphics;Utility;
StartupWMClass=BenchVault
EOF
chmod +x "$DESKTOP"

echo ">> Installed to:  $DEST_APPIMAGE"
echo ">> Runner:        $RUNNER"
echo ">> Desktop entry: $DESKTOP  (Terminal=false)"
echo ">> No-terminal launch: double-click BenchVault.desktop, or link it into the menu:"
echo "     ln -sf \"$DESKTOP\" ~/.local/share/applications/BenchVault.desktop"
