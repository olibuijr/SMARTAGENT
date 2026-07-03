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

lint_pi_imports() {
    awk '
        BEGIN { RS = ";" }
        /from[[:space:]\n\t]*["'"'"']@earendil-works\// {
            gsub(/[[:space:]\n\t]+/, " ", $0)
            if ($0 !~ /import type .*from *["'"'"']@earendil-works\//) {
                print FILENAME ":" FNR ":" $0 ";"
            }
        }
    ' "$@"
}

run_import_lint_regression() {
    DIR="$ROOT/.scratch/import-lint"
    rm -rf "$DIR"
    mkdir -p "$DIR"
    cat > "$DIR/good-type.ts" <<'EOF'
import type {
  Tool
} from "@earendil-works/pi";
EOF
    cat > "$DIR/bad-multiline.ts" <<'EOF'
import {
  defineTool
} from "@earendil-works/pi";
EOF
    cat > "$DIR/bad-odd-spacing.ts" <<'EOF'
import{typebox}from '@earendil-works/pi-tools';
EOF
    OUT=$(lint_pi_imports "$DIR"/*.ts || true)
    echo "$OUT" | grep -q 'bad-multiline.ts' || { echo "FAIL: missed multiline runtime import"; echo "$OUT"; exit 1; }
    echo "$OUT" | grep -q 'bad-odd-spacing.ts' || { echo "FAIL: missed oddly formatted runtime import"; echo "$OUT"; exit 1; }
    if echo "$OUT" | grep -q 'good-type.ts'; then echo "FAIL: false positive on multiline type import"; echo "$OUT"; exit 1; fi
    echo "ok (import lint regression)"
}

lint_extension_undefined_globals() {
    for f in "$@"; do
        [ -f "$f" ] || continue
        if grep -q '\bROOT\b' "$f" \
            && ! grep -q 'const[[:space:]]\+ROOT\b' "$f" \
            && ! grep -q 'import[^{;]*{[^}]*\bROOT\b' "$f"; then
            echo "$f: ROOT referenced but not declared/imported"
        fi
    done
}

run_extension_static_regression() {
    DIR="$ROOT/.scratch/extension-static"
    rm -rf "$DIR"
    mkdir -p "$DIR"
    cat > "$DIR/good-import.ts" <<'EOF'
import { ROOT } from "./lib/statusline-common.ts";
execFile(BIN("x"), [], { cwd: ROOT });
EOF
    cat > "$DIR/good-const.ts" <<'EOF'
const ROOT = "/repo";
execFile(BIN, [], { cwd: ROOT });
EOF
    cat > "$DIR/bad-missing-root.ts" <<'EOF'
import { BIN } from "./lib/statusline-common.ts";
execFile(BIN("x"), [], { cwd: ROOT });
EOF
    OUT=$(lint_extension_undefined_globals "$DIR"/*.ts || true)
    echo "$OUT" | grep -q 'bad-missing-root.ts' || { echo "FAIL: missed missing ROOT import"; echo "$OUT"; exit 1; }
    if echo "$OUT" | grep -q 'good-import.ts\|good-const.ts'; then echo "FAIL: false positive on declared/imported ROOT"; echo "$OUT"; exit 1; fi
    echo "ok (extension static regression)"
}

extension_tool_names() {
    # Deterministically inspect extension source instead of asking an LLM to
    # describe its tools. Each tool extension contains pi.registerTool({ name:
    # "tool" ... }); provider/command/statusline hooks do not.
    awk '
        /registerTool[[:space:]]*\(/ { in_tool = 1 }
        in_tool && match($0, /name[[:space:]]*:[[:space:]]*"[A-Za-z0-9_-]+"/) {
            s = substr($0, RSTART, RLENGTH)
            sub(/^.*:[[:space:]]*"/, "", s)
            sub(/"$/, "", s)
            print s
            in_tool = 0
        }
        in_tool && match($0, /name[[:space:]]*:[[:space:]]*\047[A-Za-z0-9_-]+\047/) {
            s = substr($0, RSTART, RLENGTH)
            sub(/^.*:[[:space:]]*\047/, "", s)
            sub(/\047$/, "", s)
            print s
            in_tool = 0
        }
        in_tool && /}\)/ { in_tool = 0 }
    ' "$@" | sort -u
}

run_tool_smoke_regression() {
    DIR="$ROOT/.scratch/tool-smoke"
    rm -rf "$DIR"
    mkdir -p "$DIR"
    cat > "$DIR/a.ts" <<'EOF'
export async function activate(pi) {
  pi.registerTool({
    name: "alpha",
    description: "test",
    parameters: { type: "object" },
    execute: async () => "ok",
  });
}
EOF
    cat > "$DIR/b.ts" <<'EOF'
export async function activate(pi) {
  pi.registerTool({ name: 'beta', description: 'test', parameters: { type: 'object' }, execute: async () => 'ok' });
}
EOF
    cat > "$DIR/prose.txt" <<'EOF'
alpha, gamma, and many other tools are available.
EOF
    OUT=$(extension_tool_names "$DIR"/*.ts)
    printf '%s\n' "$OUT" | grep -qx 'alpha' || { echo "FAIL: missed multiline registerTool name"; echo "$OUT"; exit 1; }
    printf '%s\n' "$OUT" | grep -qx 'beta' || { echo "FAIL: missed inline registerTool name"; echo "$OUT"; exit 1; }
    if printf '%s\n' "$OUT" | grep -qx 'gamma'; then echo "FAIL: parsed prose as a tool name"; echo "$OUT"; exit 1; fi
    echo "ok (tool smoke regression)"
}

run_tool_registration_smoke() {
    echo "── smoke: extension tool registry (deterministic) ──"
    run_tool_smoke_regression
    EXPECTED="semdb memory codegraph codeindex vault skills schedule search notify telegram secrets browser sa-browser orchestrate mcp sandbox context evals rag supervise tasks workflow"
    OUT=$(extension_tool_names extensions/*.ts)
    GOT=$(printf '%s\n' "$OUT" | sed '/^$/d' | wc -l | tr -d ' ')
    for name in $EXPECTED; do
        printf '%s\n' "$OUT" | grep -qx "$name" || { echo "FAIL: extension tool not registered in source: $name"; echo "$OUT"; exit 1; }
    done
    if [ "$GOT" -ne 22 ]; then echo "FAIL: expected exactly 22 active extension tools, got $GOT"; echo "$OUT"; exit 1; fi
    echo "ok ($GOT/22 extension tools registered in source)"
}

run_statusline_smoke() {
    echo "── smoke: statusline probes bounded ──"
    # Mirrors extensions/lib/statusline-common.ts. Every probe must return
    # quickly or fail clearly; a hung probe would freeze the TUI widget.
    TIMEOUT="${SMARTAGENT_STATUSLINE_TIMEOUT:-2s}"
    PROBES='codegraph|codegraph statusline data/codegraph.json
codeindex|codeindex statusline
tasks|tasks statusline --db data/tasks.semdb
workflow|workflow statusline --root . --db data/workflow.semdb
memory|memory statusline --dir data/memory
rag|rag statusline data/rag.semdb
schedule|schedule statusline
evals|evals statusline --db data/evals.semdb
orchestrate|orchestrate statusline
gateway|gateway statusline
supervise|supervise statusline
sandbox|sandbox statusline
secrets|secrets statusline --store data/secrets
browser|browser statusline
search|search statusline
hooks|hooks statusline --root .'
    printf '%s\n' "$PROBES" | while IFS='|' read -r key cmd; do
        [ -n "$key" ] || continue
        set -- $cmd
        bin="$1"; shift
        if [ ! -x "target/release/$bin" ]; then echo "FAIL: missing statusline binary $bin for $key"; exit 1; fi
        OUT=$(timeout "$TIMEOUT" "target/release/$bin" "$@" 2>&1) || { rc=$?; echo "FAIL: $key statusline rc=$rc output=$OUT"; exit 1; }
        printf '%s' "$OUT" | grep -Eq '^(ok|warn|err)\|' || { echo "FAIL: $key statusline malformed: $OUT"; exit 1; }
        echo "ok $key: $OUT"
    done
}

run_help_smoke() {
    echo "── smoke: CLI help works without required flags ──"
    OUT=$(target/release/secrets --help 2>&1) || { rc=$?; echo "FAIL: secrets --help rc=$rc output=$OUT"; exit 1; }
    printf '%s' "$OUT" | grep -q '^secrets —' || { echo "FAIL: secrets --help missing usage: $OUT"; exit 1; }
    echo "ok secrets --help"
}

restart_supervise_watch_if_running() {
    echo "── deploy: refresh supervise watch ───"
    # `supervise watch` is the one long-lived process that heals the rest.
    # After rebuilding target/release/supervise, refresh any live watch loop so
    # deploys do not leave a stale 16h-old binary supervising new services.
    if command -v systemctl >/dev/null 2>&1 && systemctl --user --quiet is-active smartagent-supervise.service 2>/dev/null; then
        systemctl --user restart smartagent-supervise.service
        echo "ok (systemd user unit restarted)"
        return 0
    fi
    PIDS=$(pgrep -f "(^|/| )target/release/supervise watch$" 2>/dev/null | tr '\n' ' ' || true)
    if [ -z "$PIDS" ]; then
        echo "ok (no supervise watch process running)"
        return 0
    fi
    for pid in $PIDS; do
        kill "$pid" 2>/dev/null || true
    done
    mkdir -p workspaces/supervise-logs
    nohup target/release/supervise watch >> workspaces/supervise-logs/watch.log 2>&1 &
    echo "ok (restarted supervise watch; old pids: $PIDS new pid: $!)"
}


run_file_and_dep_audit() {
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
}

run_hook_gate_smoke() {
    echo "── smoke: workspace write hook gate ──"
    PAYLOAD='{"tool":"write","args":{"path":"workspaces/hook-gate-empty-project/probe.txt"}}'
    set +e
    OUT=$(printf '%s' "$PAYLOAD" | hooks.d/require-doing-task.sh 2>&1)
    RC=$?
    set -e
    if [ "$RC" -ne 2 ]; then
        echo "FAIL: workspace write without project doing should block with exit 2, got $RC: $OUT"
        exit 1
    fi
    printf '%s' "$OUT" | grep -q 'tasks (project=hook-gate-empty-project)' || { echo "FAIL: block did not point at project board: $OUT"; exit 1; }
    echo "ok workspace write blocked: exit $RC"
}

gate() {
    echo "── build ──────────────────────────────"
    # desktop-agent is a separate in-progress GUI crate (own agent, own deps) —
    # excluded from this gate until it stabilizes.
    cargo build --release --workspace --exclude desktop-agent
    echo "── test ───────────────────────────────"
    cargo test --release --workspace --exclude desktop-agent
    run_file_and_dep_audit

    echo "── audit: extensions register (silent-failure guard) ──"
    # pi extensions fail SILENTLY on a bad runtime import — the tool just never
    # registers and the model hallucinates its output. Static-lint every
    # extension for the one thing that causes it: a non-type import from a pi
    # package. (Type-only `import type ... from "@earendil-works/..."` is fine.)
    run_import_lint_regression
    BADIMP=$(lint_pi_imports extensions/*.ts || true)
    if [ -n "$BADIMP" ]; then echo "FAIL: non-type pi import (extension will silently not register):"; echo "$BADIMP"; exit 1; fi
    echo "ok (all pi imports are type-only)"
    run_extension_static_regression
    BADSTATIC=$(lint_extension_undefined_globals extensions/*.ts || true)
    if [ -n "$BADSTATIC" ]; then echo "FAIL: extension undefined global reference:"; echo "$BADSTATIC"; exit 1; fi
    echo "ok (extension static globals)"

    run_tool_registration_smoke
    run_statusline_smoke
    run_help_smoke
    run_hook_gate_smoke

    restart_supervise_watch_if_running

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
    TEXT="SMARTAGENT status $TS: v$VER, $CRATES crates, gate PASS (build+test+audits+$GOT/21 tools). Latest changelog: $(sed -n '4p' CHANGELOG.md | head -c 200)"
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
    import-lint-test)
        run_import_lint_regression
        BADIMP=$(lint_pi_imports extensions/*.ts || true)
        if [ -n "$BADIMP" ]; then echo "FAIL: non-type pi import (extension will silently not register):"; echo "$BADIMP"; exit 1; fi
        run_extension_static_regression
        BADSTATIC=$(lint_extension_undefined_globals extensions/*.ts || true)
        if [ -n "$BADSTATIC" ]; then echo "FAIL: extension undefined global reference:"; echo "$BADSTATIC"; exit 1; fi
        echo "ok (current extensions lint clean)"
        ;;
    audit)
        run_file_and_dep_audit
        ;;
    tools-smoke-test)
        run_tool_registration_smoke
        ;;
    statusline-smoke-test)
        run_statusline_smoke
        ;;
    hook-gate-smoke-test)
        run_hook_gate_smoke
        ;;
    help-smoke-test)
        run_help_smoke
        ;;
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
