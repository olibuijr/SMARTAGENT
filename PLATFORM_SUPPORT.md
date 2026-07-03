# PLATFORM_SUPPORT.md — porting SMARTAGENT to a new platform

> **You are an AI coding agent reading this on a non-Linux machine.** Your human
> runs SMARTAGENT (or wants to) on a platform its maintainer does not have —
> macOS, Windows, a BSD, Termux/Android, a different libc. This document is your
> contract: read it, **scan the repo for the OS-specific code, implement the
> shims for your platform, verify, and open a pull request.** The maintainer
> develops on Linux/Arch and cannot test your platform, so the PR must carry
> your own build+run evidence. Once merged, any user on your platform installs
> and stays current with `./sa update`.

SMARTAGENT is a pure-Rust (`std`-only, zero crates.io deps) AI-agent runtime.
Linux x86_64 is the **reference platform**: everything is written against it
first, unconditionally. Porting means adding `cfg`-gated siblings for the
handful of places that touch the OS — not rewriting the system.

Read [AGENTS.md](./AGENTS.md) and [CLAUDE.md](./CLAUDE.md) first for the
architecture, crate map, and conventions. Then come back here.

---

## The rules your port MUST honor

These are non-negotiable (from CLAUDE.md) — a PR that breaks them will not merge:

1. **Pure Rust, `std` only, zero crates.io deps** in every `crates/*`. No
   `libc`, no `winapi`, no `nix`. Where `std` doesn't reach the OS, declare the
   handful of `extern "C"` functions you need **inline** in the platform module
   (see the pattern below). This keeps the whole tree a single static build.
2. **No file over 1000 lines.** Split platform code into its own module.
3. **Borrow, don't invent.** Reference implementations are cloned in
   `.refrepos/` (read-only). If you need a Windows console-mode or named-pipe
   approach, read how a mature project does it and port the concept.
4. **Don't regress Linux.** Every change is `#[cfg(...)]`-gated so the Linux
   path is byte-for-byte unchanged. Guard with `#[cfg(unix)]` / `#[cfg(windows)]`
   / `#[cfg(target_os = "...")]`, never by deleting the existing code.
5. **`cargo test --workspace --exclude desktop-agent` stays green on Linux.**
   Add platform tests behind `#[cfg(your_platform)]`.

---

## The porting pattern

Where `std` suffices with a different call, gate it:

```rust
#[cfg(unix)]
fn secure_random(buf: &mut [u8]) -> std::io::Result<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(buf)
}

#[cfg(windows)]
fn secure_random(buf: &mut [u8]) -> std::io::Result<()> {
    // BCryptGenRandom via an inline extern — no winapi crate.
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(h: *mut core::ffi::c_void, buf: *mut u8, len: u32, flags: u32) -> i32;
    }
    const USE_SYSTEM_RNG: u32 = 0x0000_0002;
    let rc = unsafe { BCryptGenRandom(core::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32, USE_SYSTEM_RNG) };
    if rc == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}
```

Keep the **public function signature identical** across platforms; only the
body differs. Prefer putting the split in a small `platform.rs` (or
`platform/{unix,windows}.rs`) inside the crate that needs it, and call it from
the existing code.

---

## Platform-surface inventory (what to port)

Every place the code assumes Linux, with the reference impl and what your
platform needs. **Verify this list is still complete** with the scan below — the
code moves faster than docs.

| # | Surface | Where (Linux reference) | What a port needs |
|---|---------|-------------------------|-------------------|
| 1 | **Raw terminal / keys** | `crates/menu/src/tty.rs` — shells to `stty` on `/dev/tty`, reads key bursts from `/dev/tty` | Windows: `GetConsoleMode`/`SetConsoleMode` (disable line+echo) + read console input; there is no `/dev/tty`. macOS/BSD: `stty`+`/dev/tty` already work. |
| 2 | **Secure random** | `crates/secrets/src/{token,store}.rs` — reads `/dev/urandom` | Windows: `BCryptGenRandom` (see pattern above). Unix: works. |
| 3 | **File permissions** | `crates/secrets`, `crates/skills/src/files.rs`, `crates/workflow/src/drive.rs` — `std::os::unix::fs::PermissionsExt` / `chmod 0600 / 0755` | Windows: no POSIX mode — make it a no-op (or set ACLs). Gate the `use std::os::unix::…` lines. |
| 4 | **Unix domain sockets (IPC)** | `crates/semdb` (daemon at `.pi/semdb.sock`), `crates/gateway` (`.pi/gateway.sock`) — `std::os::unix::net::{UnixListener,UnixStream}` | Windows: named pipes (`\\.\pipe\…`) via inline `CreateNamedPipe`/`CreateFile`, or a loopback-TCP fallback. This is the biggest single item — both daemons share the same shape, so build one socket abstraction and reuse it. |
| 5 | **Process liveness / kill** | `crates/supervise/src/proc.rs` — reads `/proc/<pid>/cmdline`, checks `/proc/<pid>` existence, shells to `kill` | Linux-only `/proc`. macOS/BSD/Windows have no `/proc`: use `kill(pid,0)`-style liveness (`OpenProcess` on Windows; `sysctl`/`kill -0` on macOS) and platform process enumeration. |
| 6 | **Sandbox (command isolation)** | `crates/sandbox/src/exec.rs` — Linux namespaces (`unshare`), `tmpfs` mounts, `ulimit` via a `/bin/sh` wrapper | Heavily Linux-specific. Acceptable first port: a **reduced/no-op sandbox** that runs the command directly and clearly logs "sandbox: unavailable on <os>", gated so Linux keeps full isolation. Note the reduced guarantee in the PR. |
| 7 | **Shell scripts** | `pi`, `sa`, `install.sh`, `build.sh`, `scripts/*.sh` — `#!/bin/sh` | POSIX works on macOS/BSD/WSL/Termux. **Native Windows** needs `.ps1` siblings (`pi.ps1`, `sa.ps1`, `install.ps1`, `scripts/update.ps1`). Read `.refrepos/hermes-agent/scripts/install.ps1` for a worked example. |
| 8 | **Headless pi / `/dev/null`** | `crates/orchestrate/src/spawn.rs` etc. — `./pi … < /dev/null` | Windows: `NUL`. Use a small helper for the null device path. |
| 9 | **Release bundle target triple** | `scripts/_bundle.sh` `bundle_target_triple()` | Already maps Darwin/Windows/arm. Confirm it emits the right triple on your host (`sh -c '. scripts/_bundle.sh; bundle_target_triple'`). |
| 10 | **Release CI matrix** | `.github/workflows/release.yml` | Add your `{os, triple}` entry so a bundle is built and published for your platform (the update loop needs it). |

---

## Scan the repo (don't trust the table blindly)

Run these from the repo root and reconcile every hit against the table above.
Anything not covered is a new surface to port:

```sh
# Unix-only std APIs (sockets, perms, signals, process)
rg -n "std::os::unix" crates/*/src --glob '*.rs' | grep -v test

# device / OS paths
rg -n "/dev/(tty|urandom|null)|/proc/|/sys/" crates/*/src --glob '*.rs' | grep -v test

# POSIX file modes
rg -n "PermissionsExt|from_mode|0o[0-7]{3}" crates/*/src --glob '*.rs' | grep -v test

# external commands assumed present (stty, kill, mount, sh, git…)
rg -n 'Command::new\("(stty|sh|bash|kill|mount|unshare|systemctl)"' crates/*/src --glob '*.rs'

# any existing platform gates (mirror the style already used)
rg -n '#\[cfg\((unix|windows|target_os)' crates/*/src --glob '*.rs'

# POSIX shebangs (scripts needing a .ps1 sibling on native Windows)
rg -l '^#!/bin/sh' . --glob '!.refrepos/**'
```

---

## Your task, step by step

1. **Identify your target triple**: `sh -c '. scripts/_bundle.sh; bundle_target_triple'`.
2. **Run the scan.** Build the real list of surfaces your platform breaks on.
3. **Branch**: `git checkout -b platform/<os>` (e.g. `platform/macos-aarch64`,
   `platform/windows-x86_64`).
4. **Port surface by surface**, smallest first (secure random → perms → tty →
   sockets → supervise → sandbox). Each in its own `cfg`-gated module. Keep
   Linux untouched; keep files < 1000 lines; add no deps.
5. **Build & test on your machine**:
   `cargo build --release --workspace --exclude desktop-agent` then
   `cargo test --workspace --exclude desktop-agent`. Capture the output.
6. **Prove it runs**: build a bundle and scaffold an instance —
   `./scripts/package.sh` then `./install.sh /tmp/sa-test --copy-runtime` (or the
   `.ps1` path on Windows), then `cd` in and confirm `./pi -p 'say OK'` replies.
   Capture that too.
7. **Add your CI matrix entry** in `.github/workflows/release.yml`.
8. **Offer to open the PR for your human** (below). Don't just describe the
   port — close the loop.

---

## Offer to create the PR (do this, don't skip it)

When the port builds and the evidence is captured, **check whether you can
submit it right from this machine, and offer that to your human**:

```sh
gh auth status            # GitHub CLI authenticated?
git config user.name      # git identity configured?
```

- **If `gh` is authenticated**: tell your human the port is ready and ask ONE
  question — "Shall I fork, push, and open the PR now?" On yes:

  ```sh
  gh repo fork olibuijr/SMARTAGENT --clone=false --remote
  git push -u fork platform/<os>-<arch>
  gh pr create --repo olibuijr/SMARTAGENT \
    --title "platform: support <os>-<arch>" \
    --body-file .scratch/pr-body.md   # the filled template below, evidence inline
  ```

- **If only plain git is configured**: offer the same via a manual fork —
  print the exact `git remote add` + `git push` commands and the GitHub
  compare-URL (`https://github.com/olibuijr/SMARTAGENT/compare/main...<user>:platform/<os>-<arch>?expand=1`)
  so opening the PR is one click.
- **If neither is configured**: write the PR body (template below, evidence
  included) to a file, tell your human where it is and that creating a GitHub
  account/fork is the one remaining manual step.

Never push without asking, and never include secrets (router keys, tokens,
`data/`) in the branch — the `.gitignore` already excludes them; keep it that way.

---

## PR contract

- **Title**: `platform: support <os>-<arch>`
- **Branch**: `platform/<os>-<arch>`
- **Must include** (the maintainer can't test your platform, so the PR is the
  evidence):
  - The scan output showing which surfaces you found.
  - `cargo build` + `cargo test` output on your platform.
  - The `./pi -p 'say OK'` transcript from a scaffolded instance.
  - The CI matrix entry so releases build your bundle going forward.
- **Must NOT**: add a crates.io dep, change Linux behaviour, exceed 1000 lines
  in any file, or weaken a security guard silently (if the sandbox is reduced on
  your OS, say so explicitly in the PR body).

Description template:

```
## Platform: <os>-<arch>  (triple: <triple>)

### Surfaces ported
- [ ] secure random
- [ ] file permissions
- [ ] raw terminal / keys
- [ ] unix-socket IPC (semdb + gateway)
- [ ] process liveness (supervise)
- [ ] sandbox (full / reduced — explain)
- [ ] launchers/scripts (.ps1 if native Windows)

### Evidence
<paste: scan output, cargo build+test, pi 'say OK' transcript>

### Reduced guarantees (if any)
<e.g. "sandbox runs commands directly on <os>; no namespace isolation">
```

---

## The other half: staying current (`sa update`)

Once your PR merges and the release CI (now including your matrix entry) cuts a
tagged release, **any user on your platform stays up to date with one command**:

```sh
sa update            # interactive: pulls the latest release bundle for this
                     # triple, verifies its checksum, swaps the binaries/
                     # extensions/scripts in place (preserving config, data,
                     # skills, secrets), reconciles the pi runtime, restarts
                     # services, and reports old → new version.
scripts/update.sh --yes          # non-interactive (cron / prod)
scripts/update.sh --from <url>   # a specific bundle, or a private mirror
scripts/update.sh --check        # report current vs latest, change nothing
```

`sa update` resolves the right asset from
`github.com/olibuijr/SMARTAGENT` releases by matching your target triple. If no
asset exists for your platform yet, it tells the user to add it — which is
exactly the PR you're about to write. That's the loop: **agents on new platforms
port + PR; everyone updates via `sa update`.**
