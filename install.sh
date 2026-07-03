#!/bin/sh
# ============================================================================
# SMARTAGENT — new isolated ./pi instance installer
# ============================================================================
# Stamps out a NEW, fully self-contained SMARTAGENT/./pi agent directory. Run
# it from ANY cwd; it resolves its own source (this repo, or a release bundle
# via --from) and scaffolds a fresh instance at <target-dir>. Every instance is
# isolated within its own dir (own binaries, config, data, sessions) so you can
# run many side by side. To UPDATE an existing instance, use `sa update`
# (scripts/update.sh) instead — this script only creates new ones.
#
#   /path/to/SMARTAGENT/install.sh <target-dir> [options]
#
# Options:
#   --name NAME          Instance label (default: basename of target-dir)
#   --from SRC           Install from a release bundle instead of this repo:
#                        a dir, a .tar.gz, or an http(s) URL to one
#   --with-source        Also copy crates/, Cargo.toml, build.sh (rebuildable;
#                        source install only, ignored with --from a bundle)
#   --build              Build release binaries in the source repo if missing
#   --copy-router-key    Copy .pi/agent/akurai-router.key from source (same
#                        machine convenience — off by default, it's a secret)
#   --copy-runtime       Copy the vendored .pi/runtime node_modules instead of
#                        running `bun install` (offline / guaranteed-pinned)
#   --force              Proceed even if <target-dir> already exists non-empty
#   -h, --help           Show this help
#
# After install: cd <target-dir> && ./sa   (first run triggers setup)
# ============================================================================
set -eu

SELF="$(cd "$(dirname "$0")" && pwd)"

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
usage() { sed -n '2,29p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

# Shared bundle library (single source of truth for instance layout).
[ -f "$SELF/scripts/_bundle.sh" ] || die "missing scripts/_bundle.sh next to installer"
# shellcheck source=scripts/_bundle.sh
. "$SELF/scripts/_bundle.sh"

# ── Parse args ──────────────────────────────────────────────────────────────
DEST=""; NAME=""; FROM=""; WITH_SOURCE=0; DO_BUILD=0; COPY_KEY=0; COPY_RUNTIME=0; FORCE=0
while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)         usage 0 ;;
        --name)            NAME="${2:?--name needs a value}"; shift 2 ;;
        --from)            FROM="${2:?--from needs a dir/tarball/url}"; shift 2 ;;
        --with-source)     WITH_SOURCE=1; shift ;;
        --build)           DO_BUILD=1; shift ;;
        --copy-router-key) COPY_KEY=1; shift ;;
        --copy-runtime)    COPY_RUNTIME=1; shift ;;
        --force)           FORCE=1; shift ;;
        -*)                die "unknown option: $1  (see --help)" ;;
        *)                 [ -z "$DEST" ] || die "unexpected argument: $1"; DEST="$1"; shift ;;
    esac
done
[ -n "$DEST" ] || { warn "no <target-dir> given"; usage 1; }

# ── Platform gate ───────────────────────────────────────────────────────────
# Linux x86_64 (glibc) is the reference platform; the prebuilt binaries are
# ELF x86-64 and will not run anywhere else. On an unsupported client, STOP
# and emit PLATFORM_SUPPORT.md — it is the porting contract, written for the
# AI coding agent sitting in front of this terminal.
# Escape hatch for porters mid-port: SMARTAGENT_SKIP_PLATFORM_CHECK=1
PLAT_OS="$(uname -s 2>/dev/null || echo unknown)"
PLAT_ARCH="$(uname -m 2>/dev/null || echo unknown)"
if [ "${SMARTAGENT_SKIP_PLATFORM_CHECK:-0}" != "1" ]; then
    case "$PLAT_OS/$PLAT_ARCH" in
        Linux/x86_64|Linux/amd64) : ;;  # supported
        *)
            warn "unsupported platform: $PLAT_OS/$PLAT_ARCH — SMARTAGENT's reference platform is Linux x86_64"
            say ""
            if [ -f "$SELF/PLATFORM_SUPPORT.md" ]; then
                say "${B}━━━ PLATFORM_SUPPORT.md ━━━${N}"
                cat "$SELF/PLATFORM_SUPPORT.md"
                say ""
                say "${B}━━━ end of PLATFORM_SUPPORT.md ━━━${N}"
                say ""
                say "If you are an AI coding agent: the document above is your porting"
                say "contract for $PLAT_OS/$PLAT_ARCH. Follow it, verify, open a PR."
                say "Porters mid-port can bypass this gate with:"
                say "  SMARTAGENT_SKIP_PLATFORM_CHECK=1 $0 ..."
            else
                warn "PLATFORM_SUPPORT.md not found next to the installer — see github.com/olibuijr/SMARTAGENT"
            fi
            die "install stopped: $PLAT_OS/$PLAT_ARCH is not supported yet"
            ;;
    esac
fi

# ── Resolve SRC (this repo, or an extracted release bundle) ─────────────────
WORK=""
if [ -n "$FROM" ]; then
    WORK="$SELF/.scratch/install-$$"
    step "fetching bundle: $FROM"
    SRC="$(bundle_fetch "$FROM" "$WORK")" || die "could not fetch bundle from $FROM"
    [ "$WITH_SOURCE" -eq 1 ] && { warn "--with-source ignored (installing from a bundle, no crates/)"; WITH_SOURCE=0; }
else
    SRC="$SELF"
fi
cleanup() { [ -n "$WORK" ] && rm -rf "$WORK"; }
trap cleanup EXIT INT TERM

# ── Resolve DEST against the invocation cwd ─────────────────────────────────
case "$DEST" in /*) : ;; *) DEST="$(pwd)/$DEST" ;; esac
DPARENT="$(cd "$(dirname "$DEST")" 2>/dev/null && pwd)" || die "parent of target does not exist: $(dirname "$DEST")"
DEST="$DPARENT/$(basename "$DEST")"
[ "$DEST" != "$SRC" ] && [ "$DEST" != "$SELF" ] || die "target dir is the source itself"
[ -n "$NAME" ] || NAME="$(basename "$DEST")"

VERSION="$(bundle_version "$SRC")"
TRIPLE="$(bundle_target_triple)"

say ""
say "${B}${C}⬡ SMARTAGENT — new ./pi instance${N}"
say "  source:  $([ -n "$FROM" ] && echo "bundle ($FROM)" || echo "$SRC") ${B}v$VERSION${N} ${D}$TRIPLE${N}"
say "  target:  $DEST"
say "  name:    $NAME"
say ""

# ── Preflight ───────────────────────────────────────────────────────────────
if [ -e "$DEST" ]; then
    if [ -d "$DEST" ] && [ -z "$(ls -A "$DEST" 2>/dev/null)" ]; then :
    elif [ "$FORCE" -eq 1 ]; then warn "target exists and is not empty — proceeding (--force)"
    else die "target exists and is not empty: $DEST  (use --force)"; fi
fi
if [ -z "$(bundle_detect_bins "$SRC")" ]; then
    if [ "$DO_BUILD" -eq 1 ] && [ -z "$FROM" ]; then
        step "building release binaries (cargo build --release)…"
        ( cd "$SRC" && cargo build --release ) || die "cargo build failed"
    else
        die "no release binaries in source — run ./build.sh first (or pass --build)"
    fi
fi

# ── Assemble ────────────────────────────────────────────────────────────────
step "copying runnable code…"
N_BINS="$(bundle_copy_code "$SRC" "$DEST")"
ok "$N_BINS tool binaries + launchers + extensions"

step "seeding per-instance state…"
bundle_seed_state "$SRC" "$DEST"
ok "config (telegram id blanked), skills, empty data/workspaces"

if [ "$WITH_SOURCE" -eq 1 ]; then
    step "copying source (crates/, Cargo.toml, build.sh)…"
    cp -Rp "$SRC/crates" "$DEST/crates"
    cp -p "$SRC/Cargo.toml" "$DEST/Cargo.toml"
    [ -f "$SRC/Cargo.lock" ] && cp -p "$SRC/Cargo.lock" "$DEST/Cargo.lock"
    cp -p "$SRC/build.sh" "$DEST/build.sh"; chmod +x "$DEST/build.sh"
fi

step "bootstrapping pi runtime…"
_mode=auto; [ "$COPY_RUNTIME" -eq 1 ] && _mode=copy
bundle_bootstrap_runtime "$SRC" "$DEST" "$_mode" || die "could not bootstrap pi runtime (try --copy-runtime)"
[ -x "$DEST/.pi/runtime/node_modules/.bin/pi" ] || die "pi binary missing after runtime bootstrap"
ok "runtime ready (pinned)"

# Router API key (secret — opt-in).
if [ "$COPY_KEY" -eq 1 ] && [ -f "$SRC/.pi/agent/akurai-router.key" ]; then
    cp -p "$SRC/.pi/agent/akurai-router.key" "$DEST/.pi/agent/akurai-router.key"
    chmod 600 "$DEST/.pi/agent/akurai-router.key"
    ok "router key copied"
fi

# Mint this instance's pi caller token (admin-gated; legitimate bootstrap).
step "minting per-instance pi caller token…"
SMARTAGENT_SECRETS_ADMIN=1 "$DEST/target/release/secrets" \
    issue-token --store "$DEST/data/secrets" --caller pi >/dev/null \
    || die "failed to mint pi caller token"
ok "token issued"

# ── Done ────────────────────────────────────────────────────────────────────
say ""
ok "${B}instance '$NAME' ready${N}  ${D}(v$VERSION)${N}"
say ""
if [ "$COPY_KEY" -eq 1 ]; then
    say "  ${B}cd $DEST && ./sa${N}      ${D}# launcher menu (already has a model key)${N}"
else
    say "  ${B}cd $DEST && ./sa${N}      ${D}# first-run setup (paste router key), then the menu${N}"
fi
say "  ${D}./pi for the agent directly · ./sa update to pull the latest release${N}"
say ""
