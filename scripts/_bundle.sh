# shellcheck shell=sh
# ============================================================================
# _bundle.sh — single source of truth for a SMARTAGENT "runnable bundle".
# ============================================================================
# Sourced (never executed) by install.sh (scaffold), package.sh (release
# tarball) and update.sh (in-place update). Defines exactly which paths make up
# the runnable CODE layer, how to copy it, how to seed fresh STATE, and how to
# name a platform. Keeping this in one place means scaffold / package / update
# can never disagree about what an instance is made of.
#
# CODE  = binaries, launchers, extensions, scripts, docs, runtime pin.
#         Overwritten on update; identical across every instance of a version.
# STATE = config, skills, data, sessions, secrets, node_modules.
#         Seeded once on scaffold; PRESERVED on update (never clobbered).
# ============================================================================

# --- platform identity ------------------------------------------------------

# Rust target triple for the current host (matches release asset names).
bundle_target_triple() {
    _os=$(uname -s 2>/dev/null || echo unknown)
    _arch=$(uname -m 2>/dev/null || echo unknown)
    case "$_arch" in
        x86_64|amd64) _arch=x86_64 ;;
        aarch64|arm64) _arch=aarch64 ;;
    esac
    case "$_os" in
        Linux)  printf '%s-unknown-linux-gnu\n' "$_arch" ;;
        Darwin) printf '%s-apple-darwin\n' "$_arch" ;;
        MINGW*|MSYS*|CYGWIN*) printf '%s-pc-windows-msvc\n' "$_arch" ;;
        *)      printf '%s-%s\n' "$_arch" "$(echo "$_os" | tr '[:upper:]' '[:lower:]')" ;;
    esac
}

# Version of a source tree / bundle: VERSION file wins (bundles ship one),
# else the workspace Cargo.toml.
bundle_version() {
    _src="$1"
    if [ -f "$_src/VERSION" ]; then
        tr -d ' \n' < "$_src/VERSION"
    elif [ -f "$_src/Cargo.toml" ]; then
        grep -m1 '^version' "$_src/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/'
    else
        echo "unknown"
    fi
}

# --- CODE layer -------------------------------------------------------------

# Executable tool binaries under target/release (no extension, excludes the
# GUI-only desktop-agent). Echoes basenames, one per line.
bundle_detect_bins() {
    _src="$1"
    for _b in "$_src"/target/release/*; do
        [ -f "$_b" ] && [ -x "$_b" ] || continue
        _base=$(basename "$_b")
        case "$_base" in *.*) continue ;; esac
        [ "$_base" = "desktop-agent" ] && continue
        printf '%s\n' "$_base"
    done
}

# Copy the CODE layer SRC -> DST. Overwrites; creates dirs as needed. Does NOT
# touch STATE. Used by scaffold, package, and update alike.
bundle_copy_code() {
    _src="$1"; _dst="$2"
    mkdir -p "$_dst/target/release" "$_dst/extensions" "$_dst/scripts" "$_dst/.pi/runtime"

    # launchers
    for _l in pi sa; do
        [ -f "$_src/$_l" ] && { cp -p "$_src/$_l" "$_dst/$_l"; chmod +x "$_dst/$_l"; }
    done
    # scripts (the bundle library + updater travel WITH the instance)
    cp -Rp "$_src/scripts/." "$_dst/scripts/"
    chmod +x "$_dst"/scripts/*.sh 2>/dev/null || true
    # extensions (thin TS glue)
    cp -Rp "$_src/extensions/." "$_dst/extensions/"
    # docs injected into agent context / used as reference
    for _d in AGENTS.md AGENT_TOOLS.md CATALOG.md PLATFORM_SUPPORT.md .gitignore; do
        [ -f "$_src/$_d" ] && cp -p "$_src/$_d" "$_dst/$_d"
    done
    # tool binaries — installed via atomic rename so replacing a RUNNING service
    # binary can't hit ETXTBSY or a torn read (rename unlinks the old inode, which
    # the live process keeps; new invocations get the new file).
    _n=0
    for _base in $(bundle_detect_bins "$_src"); do
        cp -p "$_src/target/release/$_base" "$_dst/target/release/.$_base.new"
        mv -f "$_dst/target/release/.$_base.new" "$_dst/target/release/$_base"
        _n=$((_n + 1))
    done
    # pinned pi runtime manifest (node_modules is STATE, bootstrapped separately)
    cp -p "$_src/.pi/runtime/package.json" "$_dst/.pi/runtime/package.json"
    [ -f "$_src/.pi/runtime/bun.lock" ] && cp -p "$_src/.pi/runtime/bun.lock" "$_dst/.pi/runtime/bun.lock"
    # stamp the installed version for `sa`/update to read
    bundle_version "$_src" > "$_dst/VERSION"
    echo "$_n"
}

# --- STATE layer (scaffold only) -------------------------------------------

# Seed fresh, per-instance STATE into DST from SRC. Config is copied then
# de-personalised; data/workspaces/sessions start empty. Never called on update.
bundle_seed_state() {
    _src="$1"; _dst="$2"
    mkdir -p "$_dst/config" "$_dst/data" "$_dst/workspaces" \
             "$_dst/.pi/agent" "$_dst/.pi/sessions"
    # config templates (only the tracked .conf files; no user mail/local files)
    for _cf in smartagent.conf hooks.conf; do
        [ -f "$_src/config/$_cf" ] && cp -p "$_src/config/$_cf" "$_dst/config/$_cf"
    done
    # a fresh instance must not inherit someone else's Telegram allow-id.
    # Keep this portable: BSD/GNU sed disagree on -i, and failures must be loud.
    if [ -f "$_dst/config/smartagent.conf" ]; then
        _conf="$_dst/config/smartagent.conf"
        _tmp="$_conf.tmp"
        sed 's/^telegram_allowed_chats.*/telegram_allowed_chats =/' "$_conf" > "$_tmp" || {
            rm -f "$_tmp"
            echo "bundle_seed_state: failed to de-personalize telegram_allowed_chats" >&2
            return 1
        }
        mv "$_tmp" "$_conf" || {
            rm -f "$_tmp"
            echo "bundle_seed_state: failed to install de-personalized config" >&2
            return 1
        }
        grep -qE '^telegram_allowed_chats *= *$' "$_conf" || {
            echo "bundle_seed_state: failed to blank telegram_allowed_chats" >&2
            return 1
        }
    fi
    # base skills (per-instance procedural memory accumulates from here)
    [ -d "$_src/skills" ] && cp -Rp "$_src/skills" "$_dst/skills"
    # pi agent config: default provider/model (non-secret) — needed to resolve a model
    [ -f "$_src/.pi/agent/settings.json" ] && \
        cp -p "$_src/.pi/agent/settings.json" "$_dst/.pi/agent/settings.json"
    [ -f "$_dst/.pi/agent/auth.json" ] || printf '{}' > "$_dst/.pi/agent/auth.json"
}

# --- runtime bootstrap ------------------------------------------------------

# Ensure DST/.pi/runtime/node_modules exists (the vendored pi runtime). MODE:
#   copy  -> copy node_modules from SRC (offline, guaranteed pin)
#   auto  -> bun install; fall back to copying SRC node_modules on failure
bundle_bootstrap_runtime() {
    _src="$1"; _dst="$2"; _mode="${3:-auto}"
    if [ "$_mode" = "copy" ]; then
        [ -d "$_src/.pi/runtime/node_modules" ] || return 1
        cp -Rp "$_src/.pi/runtime/node_modules" "$_dst/.pi/runtime/node_modules"
        return 0
    fi
    if command -v bun >/dev/null 2>&1 && \
       ( cd "$_dst/.pi/runtime" && bun install >/dev/null 2>&1 ); then
        return 0
    fi
    if [ -d "$_src/.pi/runtime/node_modules" ]; then
        cp -Rp "$_src/.pi/runtime/node_modules" "$_dst/.pi/runtime/node_modules"
        return 0
    fi
    return 1
}

# --- release artifact fetch -------------------------------------------------

# Resolve --from into a local extracted source dir, echoing its path. Accepts:
#   a directory  -> used as-is
#   a .tar.gz    -> extracted into WORK
#   an http(s) URL to a .tar.gz -> downloaded (curl) + extracted into WORK
# Verifies against SHA256SUMS when a sibling file/URL is available.
bundle_fetch() {
    _from="$1"; _work="$2"
    case "$_from" in
        http://*|https://*)
            command -v curl >/dev/null 2>&1 || { echo "bundle_fetch: curl required" >&2; return 1; }
            mkdir -p "$_work"
            _tar="$_work/bundle.tar.gz"
            curl -fsSL "$_from" -o "$_tar" || { echo "bundle_fetch: download failed" >&2; return 1; }
            _sums_url="$(dirname "$_from")/SHA256SUMS"
            if curl -fsSL "$_sums_url" -o "$_work/SHA256SUMS" 2>/dev/null; then
                bundle_verify "$_tar" "$_work/SHA256SUMS" || return 1
            fi
            bundle_extract "$_tar" "$_work"
            ;;
        *.tar.gz|*.tgz)
            [ -f "$_from" ] || { echo "bundle_fetch: no such file $_from" >&2; return 1; }
            mkdir -p "$_work"
            _sums="$(dirname "$_from")/SHA256SUMS"
            [ -f "$_sums" ] && { bundle_verify "$_from" "$_sums" || return 1; }
            bundle_extract "$_from" "$_work"
            ;;
        *)
            [ -d "$_from" ] || { echo "bundle_fetch: not a dir/tarball/url: $_from" >&2; return 1; }
            printf '%s\n' "$_from"
            ;;
    esac
}

bundle_extract() {
    _tar="$1"; _work="$2"
    _root="$_work/extracted"
    mkdir -p "$_root"
    tar -xzf "$_tar" -C "$_root" || { echo "bundle_extract: tar failed" >&2; return 1; }
    # bundle may have a single top dir; descend into it if so
    _inner=$(find "$_root" -mindepth 1 -maxdepth 1)
    if [ "$(printf '%s\n' "$_inner" | wc -l)" = "1" ] && [ -d "$_inner" ] && [ -f "$_inner/VERSION" ]; then
        printf '%s\n' "$_inner"
    else
        printf '%s\n' "$_root"
    fi
}

# Verify a tarball's sha256 appears in a SHA256SUMS file.
bundle_verify() {
    _tar="$1"; _sums="$2"
    command -v sha256sum >/dev/null 2>&1 || { echo "bundle_verify: sha256sum missing — skipping" >&2; return 0; }
    _have=$(sha256sum "$_tar" | awk '{print $1}')
    _base=$(basename "$_tar")
    if grep -q "$_have" "$_sums" 2>/dev/null; then
        return 0
    fi
    echo "bundle_verify: checksum mismatch for $_base" >&2
    return 1
}
