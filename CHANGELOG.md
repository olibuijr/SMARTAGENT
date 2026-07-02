# Changelog

## Unreleased — 2026-07-02 (post-v0.1.0 hardening + expansion)

**New crate:** `supervise` (20 total) — internal pure-Rust process manager that
spawns/tracks/health-checks/self-heals the long-running services (scheduler
daemon, headless chromium). Replaces per-service systemd; one optional seed unit
runs `supervise watch`.

**Security (all multi-agent-review P1s fixed):**
- Secrets: real ChaCha20-Poly1305 AEAD at rest (RFC 8439, pure std, verified against official test vectors) — replaced XOR; per-secret nonce, name as AAD, tamper detection.
- Sandbox: `env_clear` + tmpfs-masking of `data/secrets`+`.pi` inside a mount namespace (a sandboxed command can't read secrets); isolation ON by default.
- `mcp` execs argv (no `sh -c`); `schedule` arbitrary `--cmd` admin-gated (agent gets safe `--notify` only); `secrets policy-allow` admin-only + off the agent surface; web/search/rag output fenced in an UNTRUSTED envelope; `orchestrate` fork-bomb depth guard.
- httpc: single-pass reads, per-header transfer-encoding match, chunk-CRLF verify, redirect POST-body drop + cross-host auth strip.

**Architecture / rule compliance:**
- JSONL journals (schedule, evals) migrated to semdb tables — no bespoke JSONL remains.
- Dedup: one `json::Value` (semdb re-exports httpc::json), `semdb::http` is a thin httpc wrapper, `flag()`/`has()` unified into `httpc::args`, embeddings resolution centralized in `Config::embeddings()`.

**P0 fixes (12-subagent tool review, all 10 fixed):**
- secrets: caller identity is now token-authenticated — `issue-token` (admin) mints per-caller 0600 tokens; `get` requires SMARTAGENT_CALLER_TOKEN/`--token` (constant-time verify, fail-closed, audited). `./pi` launcher injects pi's token; sandbox env-scrub + secrets-mask keep it from sandboxed commands.
- mcp: JSON injection fixed (tool name escaped, --args validated as JSON).
- notify: CR/LF header injection rejected in topic/title/tags.
- codeindex: `-i` with regex now lowercases the pattern (uppercase patterns never matched).
- schedule: `--at` honors `utc_offset_minutes` config (was silently UTC); impossible dates (Feb 30) rejected instead of leaking a never-firing one-shot; new `Civil::to_unix` + `days_in_month`.
- rag: ingest deletes existing doc chunks first — re-ingest no longer leaves stale orphans.
- semdb: vector dimension enforced per-db (placeholder `[0.0]` rows exempt); mixed dims used to silently mis-score.
- search: 20s timeout (hung SearXNG no longer blocks to the 60s kill) + http(s)-only instance validation (SSRF guard).
- sandbox: ulimit caps (2GB vmem, 4096 procs, 512MB file size, CPU=timeout) in both isolation paths; isolation downgrade now warns loudly instead of degrading silently.
- browser: fixed sleeps replaced with `document.readyState` polling (`wait_ready`) in navigate/click/enter/history — faster on fast pages, no race on slow ones.

**TUI statusline (new):**
- `statusline` verb on 11 crates (supervise, sandbox, secrets, browser, search, codegraph, memory, rag, schedule, evals, orchestrate) emitting a uniform `level|icon text` protocol — severity (ok/warn/err) decided in Rust: secrets token verify, searx/chrome reachability, codegraph staleness ≥7d, memory working-cap ≥45/50, schedule soonest job ETA, evals last-run pass ratio, sandbox namespace capability.
- `extensions/statusline.ts`: per-tool footer statuses (⚙ running/✓/✗ + duration, ANSI-colored, auto-clear) via pi `tool_execution_*` events, and a two-line belowEditor widget (infra `⛭` + data `▦` rows) painting each segment green/yellow/red by level. Segments re-probe after related tool runs and every 30s. Guarded by `ctx.hasUI` (no-op headless).

**Agent capabilities & continuity:**
- Session memory: session intent captured on shutdown → episodic; recent recall injected at launch.
- Voice delisted (built but no titan STT/TTS deployed) → `extensions/disabled/`.

**Tool expansion (adversarial-council driven — features + token efficiency):**
- browser: 4→8 actions (open/click/type/back + wait/scroll/attr, `--enter`, `--quiet`, `--max-text/--max-links`).
- memory: update/recent/forget/promote + tier-scoped recall.
- search: `--time-range`/`--site`, default k=5. schedule: `--at` one-shot + pause/resume.
- rag: `--url` ingest, doc-id-scoped retrieve, get/delete-doc, `--snippet-chars`/`--ids-only`.
- vault: rm/mv (link rewrite), read `--head`. semdb: count/ids, `--ids-only`/`--meta-chars`.
- codegraph: impls + path (BFS) + `--limit`. codeindex: count/files/lines modes + `-m` cap.
- orchestrate: `out` (collect output). evals: `--min-pass`/`--fail-only`. mcp: `--names-only`/`--filter`/`--head`. sandbox: `--tail` + 16KB default.
- `./build.sh` gate now lints extensions + smoke-tests 18/18 tool registration; negative-path tests added to every CLI crate; all clippy warnings fixed.

## 0.1.0 — 2026-07-02
- Initial release: 19 pure-Rust, std-only, zero-crates.io-dep crates, each a pi extension, ported from the most popular tool in its category.
- Storage: semdb (crash-safe log store + HNSW). All data in semdb tables.
- Crates: semdb, httpc, memory, codegraph, codeindex, vault, skills, schedule, search, notify, secrets, browser (CDP), orchestrate, mcp, sandbox, context, evals, rag, voice.
- Project-isolated ./pi launcher with vendored runtime, config-driven endpoints (config/smartagent.conf), and a tools brief injected into the agent context.
