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

**TUI statusline (new):**
- `supervise statusline` verb: compact one-line `name:state` service status for UI consumption (+ unit test).
- `extensions/statusline.ts`: per-tool footer statuses (running/✓/✗ + duration, auto-clear) via pi `tool_execution_*` events + `ctx.ui.setStatus`, and a belowEditor services widget via `ctx.ui.setWidget` fed by the Rust verb. Guarded by `ctx.hasUI` (no-op headless).

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
