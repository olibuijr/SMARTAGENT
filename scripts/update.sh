#!/bin/sh
# ============================================================================
# update.sh — update THIS instance in place from a release bundle.
# ============================================================================
# Runs inside a ./pi instance (lives at <instance>/scripts/update.sh). Pulls
# the latest release for this platform (or --from a dir/tarball/url), verifies
# it, swaps the CODE layer (binaries/launchers/extensions/scripts/docs) while
# PRESERVING all STATE (config, skills, data, sessions, secrets, .pi/agent),
# reconciles the pi runtime, and restarts any running services.
#
#   scripts/update.sh                 update to the latest GitHub release
#   scripts/update.sh --from SRC      update from a dir / .tar.gz / http(s) url
#   scripts/update.sh --yes           non-interactive (no confirm prompt)
#   scripts/update.sh --check         report current vs latest, change nothing
#
# Reached interactively via `sa update`. Repo slug: $SMARTAGENT_REPO
# (default olibuijr/SMARTAGENT).
# ============================================================================
set -eu

# The instance root. Preserved across the self-detach re-exec below.
if [ -n "${SA_UPDATE_ROOT:-}" ]; then
    ROOT="$SA_UPDATE_ROOT"
else
    ROOT="$(cd "$(dirname "$0")/.." && pwd)"
fi

# Self-detach: the update overwrites scripts/ (including THIS file). Truncating
# a running script mid-execution corrupts it, so first copy ourselves + the
# bundle lib to a stable scratch dir and re-exec from there. Then it's safe to
# replace the instance's scripts/.
if [ -z "${SA_UPDATE_DETACHED:-}" ]; then
    _tmp="$ROOT/.scratch/update-self-$$"
    mkdir -p "$_tmp"
    cp "$ROOT/scripts/update.sh" "$_tmp/update.sh"
    cp "$ROOT/scripts/_bundle.sh" "$_tmp/_bundle.sh"
    SA_UPDATE_DETACHED=1 SA_UPDATE_ROOT="$ROOT" exec sh "$_tmp/update.sh" "$@"
fi

# Running from the detached copy: source its sibling lib (loaded into memory
# once, so later overwrites of the instance's copy are harmless).
# shellcheck source=_bundle.sh
. "$(dirname "$0")/_bundle.sh"

REPO="${SMARTAGENT_REPO:-olibuijr/SMARTAGENT}"
MENU="$ROOT/target/release/menu"

if [ -t 1 ]; then G='\033[0;32m'; Y='\033[0;33m'; C='\033[0;36m'; R='\033[0;31m'; B='\033[1m'; D='\033[2m'; N='\033[0m'
else G=''; Y=''; C=''; R=''; B=''; D=''; N=''; fi
step() { printf "%b→%b %s\n" "$C" "$N" "$*"; }
ok()   { printf "%b✔%b %s\n" "$G" "$N" "$*"; }
warn() { printf "%b⚠%b %s\n" "$Y" "$N" "$*"; }
die()  { printf "%b✗ %s%b\n" "$R" "$*" "$N" >&2; exit 1; }

FROM=""; YES=0; CHECK=0
while [ $# -gt 0 ]; do
    case "$1" in
        --from)  FROM="${2:?--from needs a value}"; shift 2 ;;
        --yes|-y) YES=1; shift ;;
        --check) CHECK=1; shift ;;
        -h|--help) sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
done

TRIPLE="$(bundle_target_triple)"
CUR="$(bundle_version "$ROOT")"

# ── Resolve the release URL when not given --from ───────────────────────────
resolve_latest_url() {
    command -v curl >/dev/null 2>&1 || die "curl required to reach GitHub releases (or use --from)"
    _api="https://api.github.com/repos/$REPO/releases/latest"
    _json="$(curl -fsSL "$_api" 2>/dev/null || true)"
    [ -n "$_json" ] || return 1
    # tag → version (for the check line)
    LATEST_TAG="$(printf '%s' "$_json" | grep -o '"tag_name"[^,]*' | head -1 | sed 's/.*"tag_name":[ ]*"\([^"]*\)".*/\1/')"
    # asset url matching this platform triple
    printf '%s' "$_json" \
        | grep -o '"browser_download_url":[ ]*"[^"]*"' \
        | sed 's/.*"\(https[^"]*\)"/\1/' \
        | grep -- "$TRIPLE" | grep -E '\.tar\.gz$' | head -1
}

if [ -z "$FROM" ]; then
    step "checking latest release for $TRIPLE …"
    URL="$(resolve_latest_url || true)"
    if [ -z "$URL" ]; then
        warn "no release asset for $TRIPLE at github.com/$REPO"
        warn "  (this platform may not be built yet — see PLATFORM_SUPPORT.md to add it, or use --from)"
        exit 1
    fi
    FROM="$URL"
fi

# ── Fetch + stage the new bundle ────────────────────────────────────────────
WORK="$ROOT/.scratch/update-$$"
cleanup() { rm -rf "$WORK" "$ROOT/.scratch/update-self-$$"; }
trap cleanup EXIT INT TERM

step "fetching: $FROM"
SRC="$(bundle_fetch "$FROM" "$WORK")" || die "could not fetch/verify the bundle"
NEW="$(bundle_version "$SRC")"

printf "  current: ${B}v%s${N}   available: ${B}v%s${N}\n" "$CUR" "$NEW"
if [ "$CHECK" -eq 1 ]; then
    [ "$CUR" = "$NEW" ] && ok "up to date" || warn "update available: v$CUR → v$NEW"
    exit 0
fi
if [ "$CUR" = "$NEW" ]; then
    ok "already on v$CUR — nothing to do"
    exit 0
fi

# ── Confirm ─────────────────────────────────────────────────────────────────
if [ "$YES" -ne 1 ]; then
    if [ -x "$MENU" ]; then
        "$MENU" confirm --title "Update this instance v$CUR → v$NEW?" --default yes >/dev/null || { warn "cancelled"; exit 0; }
    else
        printf "Update v%s → v%s? [Y/n]: " "$CUR" "$NEW"; read -r _a
        case "$_a" in n*|N*) warn "cancelled"; exit 0 ;; esac
    fi
fi

# Were THIS instance's services running? (its OWN semdb socket — not
# `supervise statusline`, which adopts other instances' same-named processes.)
# We deliberately DON'T restart them automatically: supervise adopts by argv[0]
# basename globally, so a `down`/`up` here could stop the source repo's or
# another instance's services. The binary swap below is atomic (rename), so the
# running services keep serving the old code safely until the user restarts.
SVC_UP=0
[ -S "$ROOT/.pi/semdb.sock" ] && SVC_UP=1

# ── Swap CODE (STATE untouched; atomic per-binary rename) ───────────────────
step "installing v$NEW …"
OLD_PIN="$(cat "$ROOT/.pi/runtime/package.json" 2>/dev/null || true)"
NB="$(bundle_copy_code "$SRC" "$ROOT")"
ok "$NB binaries + launchers + extensions + scripts updated"

# ── Reconcile pi runtime if the pin changed ─────────────────────────────────
NEW_PIN="$(cat "$ROOT/.pi/runtime/package.json" 2>/dev/null || true)"
if [ "$OLD_PIN" != "$NEW_PIN" ]; then
    step "pi runtime pin changed — reinstalling…"
    if command -v bun >/dev/null 2>&1 && ( cd "$ROOT/.pi/runtime" && bun install >/dev/null 2>&1 ); then
        ok "runtime updated"
    else
        warn "bun install failed — runtime left as-is (run: cd .pi/runtime && bun install)"
    fi
fi

# ── Service reload (manual, on purpose) ─────────────────────────────────────
# Not automated: supervise adopts by argv[0] basename globally, so restarting
# from here could disrupt another instance's services. The new binaries are in
# place; running services still hold the old (unlinked) inodes until restarted.
if [ "$SVC_UP" -eq 1 ]; then
    printf "\n  %bservices are still running the previous version.%b\n" "$Y" "$N"
    printf "  restart them to load v%s (single-instance host):\n" "$NEW"
    printf "    %bcd %s && ./target/release/supervise down && ./target/release/supervise up%b\n" "$B" "$ROOT" "$N"
fi

printf "\n%b✔ updated: v%s → v%s%b\n\n" "$G" "$CUR" "$NEW" "$N"
