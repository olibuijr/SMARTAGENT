#!/bin/sh
# ============================================================================
# package.sh — build a SMARTAGENT release bundle for the current platform.
# ============================================================================
# Produces  dist/smartagent-<version>-<target-triple>.tar.gz  and appends its
# checksum to  dist/SHA256SUMS. The bundle is a minimal source-shaped tree that
# install.sh --from and update.sh --from consume: prebuilt binaries + launchers
# + extensions + scripts + config/skills templates + the pinned runtime
# manifest (NO node_modules, NO data, NO secrets — install does `bun install`).
#
#   ./scripts/package.sh              build from an existing ./build.sh output
#   ./scripts/package.sh --build      cargo build --release first
#
# Run from the repo root (or anywhere — it resolves the repo from its path).
# ============================================================================
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=scripts/_bundle.sh
. "$ROOT/scripts/_bundle.sh"

[ "${1:-}" = "--build" ] && { echo "→ cargo build --release --workspace --exclude desktop-agent"; cargo build --release --workspace --exclude desktop-agent; }

[ -n "$(bundle_detect_bins "$ROOT")" ] || { echo "package.sh: no release binaries — run ./build.sh first (or --build)" >&2; exit 1; }

VERSION="$(bundle_version "$ROOT")"
TRIPLE="$(bundle_target_triple)"
NAME="smartagent-$VERSION-$TRIPLE"
STAGE="$ROOT/.scratch/pkg/$NAME"
DIST="$ROOT/dist"

echo "→ packaging $NAME"
rm -rf "$STAGE"; mkdir -p "$STAGE" "$DIST"

# CODE layer (binaries, launchers, extensions, scripts, docs, runtime pin, VERSION)
NB="$(bundle_copy_code "$ROOT" "$STAGE")"

# STATE seed sources the bundle must carry for install.sh --from to seed a fresh
# instance (config templates, base skills, default pi settings).
mkdir -p "$STAGE/config" "$STAGE/.pi/agent"
for cf in smartagent.conf hooks.conf; do
    [ -f "$ROOT/config/$cf" ] && cp -p "$ROOT/config/$cf" "$STAGE/config/$cf"
done
[ -d "$ROOT/skills" ] && cp -Rp "$ROOT/skills" "$STAGE/skills"
[ -f "$ROOT/.pi/agent/settings.json" ] && cp -p "$ROOT/.pi/agent/settings.json" "$STAGE/.pi/agent/settings.json"
# ship the installer so a fresh box can scaffold straight from the bundle
[ -f "$ROOT/install.sh" ] && { cp -p "$ROOT/install.sh" "$STAGE/install.sh"; chmod +x "$STAGE/install.sh"; }

# tarball (single top-level dir = $NAME)
TAR="$DIST/$NAME.tar.gz"
( cd "$ROOT/.scratch/pkg" && tar -czf "$TAR" "$NAME" )
rm -rf "$STAGE"

# checksum
if command -v sha256sum >/dev/null 2>&1; then
    ( cd "$DIST" && sha256sum "$NAME.tar.gz" >> SHA256SUMS )
    # de-dup SHA256SUMS (keep last line per file)
    awk '{ lines[$2]=$0 } END { for (f in lines) print lines[f] }' "$DIST/SHA256SUMS" | sort > "$DIST/SHA256SUMS.tmp"
    mv "$DIST/SHA256SUMS.tmp" "$DIST/SHA256SUMS"
fi

SIZE="$(du -h "$TAR" | awk '{print $1}')"
echo "✔ $TAR ($SIZE, $NB binaries)"
[ -f "$DIST/SHA256SUMS" ] && echo "✔ dist/SHA256SUMS updated"
