#!/usr/bin/env bash
# Publish the current version's bundles to the Codeberg (Forgejo) package
# registry and link each package to the repository.
#
#   .deb       -> debian registry   (apt)
#   .rpm       -> rpm registry      (dnf/yum)
#   .AppImage  -> generic registry
#
# Docs: https://forgejo.org/docs/latest/user/packages/
#
# Usage:
#   CODEBERG_TOKEN=... scripts/publish-to-codeberg.sh             # build, upload, link
#   CODEBERG_TOKEN=... scripts/publish-to-codeberg.sh --no-build  # upload existing bundles
#   CODEBERG_TOKEN=... scripts/publish-to-codeberg.sh --link-only # only (re)link packages
#
# Environment:
#   CODEBERG_TOKEN            required — personal access token with package:write
#   CODEBERG_USER             basic-auth user (default: owner)
#   CODEBERG_HOST             default: codeberg.org
#   CODEBERG_OWNER/REPO       default: parsed from the `origin` remote
#   DEB_DISTRIBUTION          default: stable
#   DEB_COMPONENT             default: main
#   RPM_GROUP                 default: stable
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE_DIR="$REPO_ROOT/src-tauri/target/release/bundle"

# Load .env from the repo root (variables there may lack `export`).
if [[ -f "$REPO_ROOT/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "$REPO_ROOT/.env"
  set +a
fi

BUILD=1
UPLOAD=1
case "${1:-}" in
  --no-build)  BUILD=0 ;;
  --link-only) BUILD=0; UPLOAD=0 ;;
esac

: "${CODEBERG_TOKEN:?Set CODEBERG_TOKEN to a token with package:write scope}"
HOST="${CODEBERG_HOST:-codeberg.org}"

# owner/repo from ssh://git@host/owner/repo.git or git@host:owner/repo.git
remote="$(git -C "$REPO_ROOT" remote get-url origin 2>/dev/null || true)"
slug="${remote#*"$HOST"}"; slug="${slug#[:/]}"; slug="${slug%.git}"
OWNER="${CODEBERG_OWNER:-${slug%%/*}}"
REPO="${CODEBERG_REPO:-${slug##*/}}"
[[ -n "$OWNER" && -n "$REPO" && "$OWNER" != "$REPO" ]] \
  || { echo "!! Cannot derive owner/repo from '$remote'; set CODEBERG_OWNER and CODEBERG_REPO" >&2; exit 1; }
USER_NAME="${CODEBERG_USER:-$OWNER}"

DEB_DISTRIBUTION="${DEB_DISTRIBUTION:-stable}"
DEB_COMPONENT="${DEB_COMPONENT:-main}"
RPM_GROUP="${RPM_GROUP:-stable}"

VERSION="$(node -p "require('$REPO_ROOT/src-tauri/tauri.conf.json').version")"
PKG_VERSION="$(node -p "require('$REPO_ROOT/package.json').version")"
CARGO_VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO_ROOT/src-tauri/Cargo.toml" | head -n1)"
if [[ "$VERSION" != "$PKG_VERSION" || "$VERSION" != "$CARGO_VERSION" ]]; then
  echo "!! Version mismatch: tauri.conf.json=$VERSION package.json=$PKG_VERSION Cargo.toml=$CARGO_VERSION" >&2
  exit 1
fi

echo ">> Publishing BenchVault $VERSION to https://$HOST/$OWNER/$REPO"

if [[ $BUILD -eq 1 ]]; then
  echo ">> Building bundles..."
  ( cd "$REPO_ROOT" && npm run tauri build )
fi

# Curl auth via stdin config so the token never lands in the process list.
auth_config() { printf 'user = "%s:%s"\n' "$USER_NAME" "$CODEBERG_TOKEN"; }

RESPONSE="$(mktemp)"
trap 'rm -f "$RESPONSE"' EXIT

# upload <file> <url>
upload() {
  local file="$1" url="$2" code
  echo ">> Uploading $(basename "$file")"
  code="$(auth_config | curl -sS -o "$RESPONSE" -w '%{http_code}' \
    --config - --upload-file "$file" "$url")"
  case "$code" in
    201|202) echo "   ok" ;;
    409)     echo "   already published — skipping" ;;
    *) echo "!! HTTP $code" >&2; sed -e 's/^/   /' "$RESPONSE" >&2; exit 1 ;;
  esac
}

# registry_name <type> — name the registry stored for this version (rpm appends a
# release to the version, so match on the prefix).
registry_name() {
  local type="$1"
  auth_config | curl -sS --config - "https://$HOST/api/v1/packages/$OWNER?type=$type" \
    | node -e '
      let s = ""; process.stdin.on("data", d => s += d).on("end", () => {
        const hit = JSON.parse(s).find(p => p.version.startsWith(process.argv[1]));
        if (!hit) { process.exit(1); }
        console.log(hit.name);
      });' "$VERSION"
}

linked_repo() {
  local type="$1" name="$2"
  auth_config | curl -sS --config - "https://$HOST/api/v1/packages/$OWNER?type=$type" \
    | node -e '
      let s = ""; process.stdin.on("data", d => s += d).on("end", () => {
        const hit = JSON.parse(s).find(p => p.name === process.argv[1]
          && p.version.startsWith(process.argv[2]));
        console.log(hit && hit.repository ? hit.repository.name : "");
      });' "$name" "$VERSION"
}

link() {
  local type="$1" name="$2" code current
  current="$(linked_repo "$type" "$name")"
  if [[ "${current,,}" == "${REPO,,}" ]]; then
    echo ">> $type/$name already linked to $OWNER/$current"
    return
  fi
  code="$(auth_config | curl -sS -o /dev/null -w '%{http_code}' --config - -X POST \
    "https://$HOST/api/v1/packages/$OWNER/$type/$name/-/link/$REPO")"
  case "$code" in
    201|204) echo ">> Linked $type/$name to $OWNER/$REPO" ;;
    *)       echo "!! Link $type/$name failed: HTTP $code" >&2; exit 1 ;;
  esac
}

# find_bundle <subdir> <glob>
find_bundle() {
  local hit
  hit="$(ls -t "$BUNDLE_DIR/$1"/$2 2>/dev/null | head -n1 || true)"
  [[ -n "$hit" ]] || { echo "!! No $2 found in $BUNDLE_DIR/$1 — run without --no-build" >&2; exit 1; }
  echo "$hit"
}

GENERIC_NAME="benchvault"

if [[ $UPLOAD -eq 1 ]]; then
  DEB="$(find_bundle deb "*_${VERSION}_*.deb")"
  RPM="$(find_bundle rpm "*-${VERSION}-*.rpm")"
  APPIMAGE="$(find_bundle appimage "*_${VERSION}_*.AppImage")"

  upload "$DEB" "https://$HOST/api/packages/$OWNER/debian/pool/$DEB_DISTRIBUTION/$DEB_COMPONENT/upload"
  upload "$RPM" "https://$HOST/api/packages/$OWNER/rpm/$RPM_GROUP/upload"
  upload "$APPIMAGE" "https://$HOST/api/packages/$OWNER/generic/$GENERIC_NAME/$VERSION/$(basename "$APPIMAGE")"
fi

# The deb/rpm names are set by the package metadata — ask the registry rather
# than guessing them from the file names.
link debian "$(registry_name debian)"
link rpm "$(registry_name rpm)"
link generic "$GENERIC_NAME"

echo ">> Done. Packages: https://$HOST/$OWNER/-/packages"
