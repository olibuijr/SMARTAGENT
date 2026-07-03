#!/bin/sh
# ============================================================================
# SMARTAGENT — new isolated ./pi instance installer
# ============================================================================
# Stamps out a NEW, fully self-contained SMARTAGENT/./pi agent directory.
# Run it from ANY cwd; it resolves its own source repo and scaffolds a fresh
# instance at <target-dir>. Every instance is isolated within its own dir
# (own binaries, config, data, sessions, workspaces) so you can run many
# side by side without them touching each other.
#
#   /path/to/SMARTAGENT/install.sh <target-dir> [options]
#
# Options:
#   --name NAME          Instance label (default: basename of target-dir)
#   --with-source        Also copy crates/, Cargo.toml, build.sh (rebuildable)
#   --build              Build release binaries in the source repo if missing
#   --copy-router-key    Copy .pi/agent/akurai-router.key from source (same
#                        machine convenience — off by default, it's a secret)
#   --copy-runtime       Copy the vendored .pi/runtime node_modules instead of
#                        running `bun install` (offline / guaranteed-pinned)
#   --force              Proceed even if <target-dir> already exists non-empty
#   -h, --help           Show this help
#
# What a fresh instance contains:
#   pi  extensions/  target/release/<tools>  config/  skills/  AGENTS.md
#   AGENT_TOOLS.md  CATALOG.md  .pi/runtime  data/  workspaces/  .gitignore
# Then: cd <target-dir> && ./pi
# ============================================================================
set -eu

# ── Colors ──────────────────────────────────────────────────────────────────
if [ -t 1 ]; then
    G='\033[0;32m'; Y='\033[0;33m'; C='\033[0;36m'; R='\033[0;31m'; B='\033[1m'; D='\033[2m'; N='\033[0m'
else
    G=''; Y=''; C=''; R=''; B=''; D=''; N=''
fi
say()  { printf "%b\n" "$*"; }
step() { printf "%b→%b %s\n" "$C" "$N" "$*"; }
ok()   { printf "%b✔%b %s\n" "$G" "$N" "$*"; }
warn() { printf "%b⚠%b %s\n" "$Y" "$N" "$*"; }
die()  { printf "%b✗ %s%b\n" "$R" "$*" "$N" >&2; exit 1; }

# ── Source repo = where this script lives (cwd-independent) ─────────────────
SRC="$(cd "$(dirname "$0")" && pwd)"

usage() { sed -n '2,33p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

# ── Parse args ──────────────────────────────────────────────────────────────
DEST=""; NAME=""; WITH_SOURCE=0; DO_BUILD=0; COPY_KEY=0; COPY_RUNTIME=0; FORCE=0
while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)        usage 0 ;;
        --name)           NAME="${2:?--name needs a value}"; shift 2 ;;
        --with-source)    WITH_SOURCE=1; shift ;;
        --build)          DO_BUILD=1; shift ;;
        --copy-router-key) COPY_KEY=1; shift ;;
        --copy-runtime)   COPY_RUNTIME=1; shift ;;
        --force)          FORCE=1; shift ;;
        -*)               die "unknown option: $1  (see --help)" ;;
        *)                [ -z "$DEST" ] || die "target dir already given ($DEST); unexpected: $1"
                          DEST="$1"; shift ;;
    esac
done
[ -n "$DEST" ] || { warn "no <target-dir> given"; usage 1; }

# ── Resolve target dir against the invocation cwd ───────────────────────────
case "$DEST" in /*) : ;; *) DEST="$(pwd)/$DEST" ;; esac
# Normalize without requiring the dir to exist yet.
DEST_PARENT="$(cd "$(dirname "$DEST")" 2>/dev/null && pwd)" || die "parent of target does not exist: $(dirname "$DEST")"
DEST="$DEST_PARENT/$(basename "$DEST")"
[ "$DEST" != "$SRC" ] || die "target dir is the source repo itself"
[ -n "$NAME" ] || NAME="$(basename "$DEST")"

VERSION="$(grep -m1 '^version' "$SRC/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"

say ""
say "${B}${C}⬡ SMARTAGENT — new ./pi instance${N}"
say "  source:  $SRC ${B}(v$VERSION)${N}"
say "  target:  $DEST"
say "  name:    $NAME"
[ "$WITH_SOURCE" -eq 1 ] && say "  mode:    with-source (rebuildable)"
say ""

# ── Preflight ───────────────────────────────────────────────────────────────
if [ -e "$DEST" ]; then
    if [ -d "$DEST" ] && [ -z "$(ls -A "$DEST" 2>/dev/null)" ]; then
        : # empty dir, fine
    elif [ "$FORCE" -eq 1 ]; then
        warn "target exists and is not empty — proceeding (--force)"
    else
        die "target exists and is not empty: $DEST  (use --force to override)"
    fi
fi

# Binaries: copy every top-level executable in target/release (no extension),
# skipping desktop-agent (GUI, not needed for a headless ./pi instance).
have_bins() { [ -x "$SRC/target/release/secrets" ] && [ -x "$SRC/target/release/gateway" ]; }
if ! have_bins; then
    if [ "$DO_BUILD" -eq 1 ]; then
        step "building release binaries in source (cargo build --release)…"
        ( cd "$SRC" && cargo build --release ) || die "cargo build failed"
    else
        die "release binaries missing in $SRC/target/release — run ./build.sh first, or pass --build"
    fi
fi

# ── Scaffold skeleton ───────────────────────────────────────────────────────
step "creating instance skeleton…"
mkdir -p "$DEST/target/release" "$DEST/config" "$DEST/extensions" \
         "$DEST/.pi/runtime" "$DEST/.pi/agent" "$DEST/.pi/sessions" \
         "$DEST/data" "$DEST/workspaces"

# ── Launcher + docs (verbatim; all ROOT-relative, portable as-is) ───────────
step "copying launcher, extensions, docs…"
cp -p "$SRC/pi" "$DEST/pi"; chmod +x "$DEST/pi"
[ -f "$SRC/sa" ] && { cp -p "$SRC/sa" "$DEST/sa"; chmod +x "$DEST/sa"; }
cp -Rp "$SRC/extensions/." "$DEST/extensions/"
for f in AGENTS.md AGENT_TOOLS.md CATALOG.md; do
    [ -f "$SRC/$f" ] && cp -p "$SRC/$f" "$DEST/$f"
done
[ -f "$SRC/.gitignore" ] && cp -p "$SRC/.gitignore" "$DEST/.gitignore"
[ -d "$SRC/skills" ] && cp -Rp "$SRC/skills" "$DEST/skills"

# ── Config: copy the tracked .conf files; blank the personal Telegram id ────
step "seeding config (per-instance)…"
for cf in smartagent.conf hooks.conf; do
    [ -f "$SRC/config/$cf" ] && cp -p "$SRC/config/$cf" "$DEST/config/$cf"
done
if [ -f "$DEST/config/smartagent.conf" ]; then
    sed -i 's/^telegram_allowed_chats.*/telegram_allowed_chats =/' "$DEST/config/smartagent.conf"
fi

# ── Tool binaries ───────────────────────────────────────────────────────────
step "copying tool binaries…"
n=0
for bin in "$SRC"/target/release/*; do
    [ -f "$bin" ] && [ -x "$bin" ] || continue
    base="$(basename "$bin")"
    case "$base" in *.* ) continue ;; esac        # skip *.d / *.so / etc.
    [ "$base" = "desktop-agent" ] && continue      # GUI, not needed headless
    cp -p "$bin" "$DEST/target/release/$base"
    n=$((n + 1))
done
ok "$n tool binaries"

# ── Optional source (rebuildable dev instance) ──────────────────────────────
if [ "$WITH_SOURCE" -eq 1 ]; then
    step "copying source (crates/, Cargo.toml, build.sh)…"
    cp -Rp "$SRC/crates" "$DEST/crates"
    cp -p "$SRC/Cargo.toml" "$DEST/Cargo.toml"
    [ -f "$SRC/Cargo.lock" ] && cp -p "$SRC/Cargo.lock" "$DEST/Cargo.lock"
    cp -p "$SRC/build.sh" "$DEST/build.sh"; chmod +x "$DEST/build.sh"
fi

# ── Vendored pi runtime (.pi/runtime) ───────────────────────────────────────
cp -p "$SRC/.pi/runtime/package.json" "$DEST/.pi/runtime/package.json"
[ -f "$SRC/.pi/runtime/bun.lock" ] && cp -p "$SRC/.pi/runtime/bun.lock" "$DEST/.pi/runtime/bun.lock"
if [ "$COPY_RUNTIME" -eq 1 ]; then
    step "copying vendored pi runtime (node_modules)…"
    [ -d "$SRC/.pi/runtime/node_modules" ] || die "no node_modules in source to copy"
    cp -Rp "$SRC/.pi/runtime/node_modules" "$DEST/.pi/runtime/node_modules"
    ok "runtime copied (pinned)"
else
    step "installing pinned pi runtime (bun install)…"
    if command -v bun >/dev/null 2>&1 && ( cd "$DEST/.pi/runtime" && bun install >/dev/null 2>&1 ); then
        ok "runtime installed via bun"
    elif [ -d "$SRC/.pi/runtime/node_modules" ]; then
        warn "bun install unavailable/failed — copying source node_modules instead"
        cp -Rp "$SRC/.pi/runtime/node_modules" "$DEST/.pi/runtime/node_modules"
        ok "runtime copied (pinned)"
    else
        die "could not install pi runtime: no bun and no source node_modules (try --copy-runtime)"
    fi
fi
[ -x "$DEST/.pi/runtime/node_modules/.bin/pi" ] || die "pi binary missing after runtime bootstrap"

# ── pi agent config (non-secret: default provider/model/theme) ──────────────
# Needed for the instance to resolve a model at all — without settings.json pi
# falls back to a bogus default provider. auth.json (OAuth creds) copied only
# if present and non-empty is left to the user; we seed an empty one.
step "seeding pi agent config…"
if [ -f "$SRC/.pi/agent/settings.json" ]; then
    cp -p "$SRC/.pi/agent/settings.json" "$DEST/.pi/agent/settings.json"
    ok "default provider/model config"
else
    warn "no source settings.json — instance will use pi's built-in defaults"
fi
[ -f "$DEST/.pi/agent/auth.json" ] || printf '{}' > "$DEST/.pi/agent/auth.json"

# ── Router API key (secret — opt-in) ────────────────────────────────────────
if [ "$COPY_KEY" -eq 1 ]; then
    if [ -f "$SRC/.pi/agent/akurai-router.key" ]; then
        step "copying router key…"
        cp -p "$SRC/.pi/agent/akurai-router.key" "$DEST/.pi/agent/akurai-router.key"
        chmod 600 "$DEST/.pi/agent/akurai-router.key"
        ok "router key copied"
    else
        warn "no router key in source to copy"
    fi
fi

# ── Mint this instance's pi caller token ────────────────────────────────────
step "minting per-instance pi caller token…"
# issue-token is admin-gated; scaffolding an instance's own token is a
# legitimate one-shot admin bootstrap, so grant it for just this call.
SMARTAGENT_SECRETS_ADMIN=1 "$DEST/target/release/secrets" \
    issue-token --store "$DEST/data/secrets" --caller pi >/dev/null \
    || die "failed to mint pi caller token"
ok "token issued (data/secrets/tokens/pi.token)"

# ── Done ────────────────────────────────────────────────────────────────────
say ""
ok "${B}instance '$NAME' ready${N}"
say ""
if [ "$COPY_KEY" -eq 1 ]; then
    say "  ${B}cd $DEST && ./sa${N}      ${D}# launcher menu (already has a model key)${N}"
else
    say "  ${B}cd $DEST && ./sa${N}      ${D}# runs first-run setup (paste your router key), then the menu${N}"
fi
say "  ${D}or ./pi for the agent directly · ./sa setup to reconfigure${N}"
say ""
