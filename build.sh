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
    # desktop-agent is a separate in-progress GUI crate (own agent, own deps) —
    # excluded from this gate until it stabilizes.
    cargo build --release --workspace --exclude desktop-agent
    echo "── test ───────────────────────────────"
    cargo test --release --workspace --exclude desktop-agent
    echo "── audit: no file > 1000 lines ────────"
    BIG=$(find crates -name '*.rs' -exec wc -l {} + | awk '$1>1000{print}' | grep -v ' total$' || true)
    if [ -n "$BIG" ]; then echo "FAIL: files over 1000 lines:"; echo "$BIG"; exit 1; fi
    echo "ok"
    echo "── audit: zero crates.io deps ─────────"
    # A crates.io dep is `name = "<version>"` (version starts with a digit or ^~*).
    # Path deps `name = { path = ... }` and the [package] name/edition are exempt.
    DEP=$(grep -rEl '^[a-z0-9_-]+ = "[0-9^~*<>=]' crates/*/Cargo.toml 2>/dev/null || true)
    if [ -n "$DEP" ]; then echo "FAIL: crates.io deps found in:"; echo "$DEP"; exit 1; fi
    echo "ok (path deps only)"

    echo "── audit: extensions register (silent-failure guard) ──"
    # pi extensions fail SILENTLY on a bad runtime import — the tool just never
    # registers and the model hallucinates its output. Static-lint every
    # extension for the one thing that causes it: a non-type import from a pi
    # package. (Type-only `import type ... from "@earendil-works/..."` is fine.)
    BADIMP=$(grep -rn "from ['\"]@earendil-works/" extensions/*.ts | grep -v 'import type ' || true)
    if [ -n "$BADIMP" ]; then echo "FAIL: non-type pi import (extension will silently not register):"; echo "$BADIMP"; exit 1; fi
    echo "ok (all pi imports are type-only)"

    echo "── smoke: pi loads and every extension tool registers ──"
    # 20 active tools; `voice` is built but delisted (extensions/disabled/) until
    # a titan STT/TTS server exists. The check asks the model to list its tools,
    # which is mildly non-deterministic (an LLM may occasionally omit one name),
    # so retry a few times and pass if any attempt sees all 20.
    RE='\b(semdb|memory|codegraph|codeindex|vault|skills|schedule|search|notify|secrets|browser|orchestrate|mcp|sandbox|context|evals|rag|supervise|tasks|workflow)\b'
    GOT=0
    for attempt in 1 2 3; do
        GOT=$(./pi -p 'List every tool you can call, names only, comma-separated. No prose.' </dev/null 2>/dev/null \
            | grep -oE "$RE" | sort -u | wc -l | tr -d ' ')
        [ "$GOT" -ge 20 ] && break
        echo "  attempt $attempt: $GOT/20 — retrying"
    done
    echo "tools the agent listed: $GOT / 20"
    if [ "$GOT" -lt 20 ]; then echo "FAIL: only $GOT/20 crate tools registered in pi"; exit 1; fi
    echo "ok ($GOT/20 crate tools live)"

    echo "── status: project memory snapshot ────"
    update_status
}

# Autonomous per-build status capture: one row per gate pass into the
# project-scoped semdb (.smartagent/semdb/project.semdb — the repo memory
# convention), plus a rolling `status-latest`. Embeds when the embeddings
# endpoint is reachable so the row is semantically recallable; falls back to a
# placeholder vector (still stored) when offline. Never fails the build.
update_status() {
    SDB=".smartagent/semdb/project.semdb"
    mkdir -p .smartagent/semdb
    [ -f "$SDB" ] || ./target/release/semdb create "$SDB" >/dev/null 2>&1 || true
    VER=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
    CRATES=$(ls crates | wc -l | tr -d ' ')
    TS=$(date -u +%Y-%m-%dT%H:%MZ)
    TEXT="SMARTAGENT status $TS: v$VER, $CRATES crates, gate PASS (build+test+audits+$GOT/20 tools). Latest changelog: $(sed -n '4p' CHANGELOG.md | head -c 200)"
    EMBED_TIMEOUT="${SMARTAGENT_STATUS_EMBED_TIMEOUT:-15s}"
    if timeout "$EMBED_TIMEOUT" ./target/release/semdb embed "$SDB" --id "status-latest" --text "$TEXT" >/dev/null 2>&1; then
        timeout "$EMBED_TIMEOUT" ./target/release/semdb embed "$SDB" --id "status-$TS" --text "$TEXT" >/dev/null 2>&1 || true
        echo "ok (embedded status row → $SDB)"
    else
        ./target/release/semdb put "$SDB" --id "status-latest" --vector 0 --meta "{\"text\":\"$TEXT\"}" >/dev/null 2>&1 || true
        echo "ok (placeholder status row — embeddings endpoint unreachable)"
    fi
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
