#!/bin/sh
# SMARTAGENT build/release gate — versioning + changelog + build + test.
#
#   ./build.sh                 build + test the whole workspace (no version bump)
#   ./build.sh patch|minor|major "<changelog line>"   bump version, changelog, gate
#
# Version lives in Cargo.toml [workspace.package]; all crates inherit it.
set -e
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

CUR=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')

gate() {
    echo "── build ──────────────────────────────"
    cargo build --release --workspace
    echo "── test ───────────────────────────────"
    cargo test --release --workspace
    echo "── audit: no file > 1000 lines ────────"
    BIG=$(find crates -name '*.rs' -exec wc -l {} + | awk '$1>1000{print}' | grep -v ' total$' || true)
    if [ -n "$BIG" ]; then echo "FAIL: files over 1000 lines:"; echo "$BIG"; exit 1; fi
    echo "ok"
    echo "── audit: zero crates.io deps ─────────"
    DEP=$(grep -rEl '^[a-z0-9_-]+ = "' crates/*/Cargo.toml 2>/dev/null || true)
    if [ -n "$DEP" ]; then echo "FAIL: crates.io deps found in:"; echo "$DEP"; exit 1; fi
    echo "ok (path deps only)"
}

case "${1:-}" in
    patch|minor|major)
        MSG="${2:?changelog line required: ./build.sh $1 \"message\"}"
        IFS=. read -r MA MI PA <<EOF
$CUR
EOF
        case "$1" in
            major) MA=$((MA+1)); MI=0; PA=0 ;;
            minor) MI=$((MI+1)); PA=0 ;;
            patch) PA=$((PA+1)) ;;
        esac
        NEW="$MA.$MI.$PA"
        gate
        sed -i "0,/^version = \"$CUR\"/s//version = \"$NEW\"/" Cargo.toml
        DATE=$(date +%Y-%m-%d)
        TMP=$(mktemp -p "$ROOT/.scratch" 2>/dev/null || echo "$ROOT/.scratch/CL.tmp")
        mkdir -p "$ROOT/.scratch"
        {
            echo "# Changelog"; echo
            echo "## $NEW — $DATE"; echo "- $MSG"; echo
            tail -n +2 CHANGELOG.md 2>/dev/null || true
        } > "$TMP"
        mv "$TMP" CHANGELOG.md
        echo "── version $CUR → $NEW, CHANGELOG updated ──"
        ;;
    "")
        gate
        ;;
    *)
        echo "usage: ./build.sh [patch|minor|major \"<changelog line>\"]"; exit 1 ;;
esac
