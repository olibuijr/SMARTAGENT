# Changelog

## Unreleased
- **Enforced agent operating loop:** AGENT_TOOLS.md "How to work" replaced by a 7-step ordered loop (skills match → tasks pull → workflow task-run → investigate cheap→expensive → execute → verify → close), stated once and backed by deterministic hooks — `require-doing-task` blocks pi's edit/write while nothing is in `doing` (root board or the target workspace repo's own board; `.scratch/` + `SMARTAGENT_HOOKS_RELAX=1` exempt; block message = exact unlock commands), `session-brief` injects live board/workflow/index state at agent start, `stop-board-audit` snapshots the board into the hooks audit trail. Fixed latent `hooks.ts` crash: `before_agent_start` context injection returned the wrong shape ("content is not iterable") — never exercised until a session_start hook existed.
- **Workspace project indexing (codeindex):** every repo under `workspaces/` is a first-class project — `projects` lists repos + index status, `index <name>|--all` builds a per-repo file inventory at `<repo>/.smartagent/codeindex.semdb` (structural semdb rows, no embeddings needed) with per-repo OK/FAIL + skipped-non-repo reporting; `search`/`files --project <name>` scope to one repo using its own .gitignore. `.smartagent` added to the ALWAYS-skip list; latent `positional_dir` bug fixed (flag values mistaken for the search dir). All 27 workspace repos indexed 27 ok / 0 failed via a live `./pi` run.
- **Per-project scoping across tools (`--project <name>` / `project` param):** shared `semdb::workspace` module (root/list/resolve/data_path, traversal-guarded) resolves `<repo>/.smartagent/<store>` — per-repo kanban boards (`tasks.semdb`: tasks never mix between repos, per-repo WIP), per-repo code graphs (`codegraph.json`: fixes silent clobbering of the single global slot; 0-symbol note for non-Rust repos), per-repo memory (`memory/`: memory-policy alignment), per-repo rag corpora (`rag.semdb`: friendly no-corpus error), per-repo workflow run state (`workflow.semdb`: pairs with the repo's board). Kept host-global by design: vault, evals, schedule, secrets, session intents.
- **semdb `put_many`:** bulk insert with a single fsync (per-put `sync_data` = one fsync per row — minutes for a large code index).
- **Statusline v2:** widget refactored from 2 crowded lines to 3 scope-grouped lines — ⌂ workspace (Code, Index repos+files, Tasks, Workflow) / ▦ data (Memory, Docs, Schedule, Evals, Agents) / ⛭ infra (Services, Sandbox, Secrets, Browser, Search, Hooks); new `codeindex statusline` verb (`ok|🗃 27/27 repos, 2554 files`, warns on unindexed/stale); segments column-aligned via wcwidth-accurate padding (text-presentation glyphs like 🕸/🗃 are 1 cell in kitty); infra health segments collapse to `Name ✓` when ok while stat segments keep details; tasks statusline text humanized (`0/1 doing · 2 ready · 1 blocked`).
- `.smartagent/` added to the user's global gitignore so per-repo state dirs never dirty workspace repos' `git status`.
- Embeddings + voice endpoints wired to the akurai-vpn overlay (titan at 100.88.0.2:8081) — works off-LAN; LAN address 192.168.1.119 remains as fallback knowledge in docs.
- build.sh gate now writes an autonomous per-build status snapshot into the project semdb (.smartagent/semdb/project.semdb): embedded when the endpoint is up, placeholder row offline, never fails the build.

## 0.2.0 — 2026-07-02
- Tool review P0/P1 complete: secrets caller-token auth, hooks system, kanban tasks + workflow engine, colored statusline, batch embeddings, hardened sandbox/search/mcp/notify

### Detail (post-v0.1.0 hardening + expansion)

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

**Task management + process engine (PAI patterns as pure Rust, 2 new crates → 22 total, 20 active tools):**
- `tasks` crate: kanban board in a semdb table — backlog→ready→doing→review→done, WIP limits (doing=1 default) and criteria-gated `done` ENFORCED in Rust, pull-based `next`, explicit blockers with reasons, cycle-time/lead-time/throughput `metrics`, `statusline`. Extension `tasks.ts`.
- `workflow` crate: markdown-defined process engine — steps in `workflows/*.md` + `skills/*/Workflows/*.md` declare `skill:` (PAI skill-per-phase routing) and `expect:`; `advance` REQUIRES evidence (trivial 'done'/'ok' rejected — the inline-verification mandate, deterministic). Run state in semdb; `statusline` warns on runs stalled >1d. Extension `workflow.ts`.
- `skills match`: prompt-scored skill auto-trigger (word-boundary token overlap, name hits 3×) — picks the right skill for a task/step, unlike single-substring `search`.
- `skills/Kanban`: methodology skill (six practices, column semantics, blocker/priority policy, anti-patterns) + three runnable workflows: `task-run` (observe→plan→execute→verify→learn, one skill per step), `triage`, `retro`.
- Gate updated to 20/20 tool registration; statusline widget gained ▣ tasks + ▶ workflow segments.

**Hooks system (new crate, 23 total):** `hooks` — user-configurable lifecycle hooks researched from Claude Code + codex CLI (3-agent research team). Config `config/hooks.conf` (additive [hook] blocks: event/matcher/command/timeout), commands in `hooks.d/`, Claude Code I/O contract (payload JSON on stdin; exit 0 allow with optional {"decision":"block"}/{"updatedInput"}/plain-text context; exit 2 BLOCK with stderr reason; timeout fails open loudly). `extensions/hooks.ts` bridges pi events: tool_call (block+rewrite), input (user_prompt block), before_agent_start (context injection), agent_end (stop audit). Firings audited to data/hooks.semdb; 🪝 statusline segment; seeded with a guard-destructive hook on sandbox — verified blocking `rm -rf /` end-to-end through pi.

**P1 backlog burn-down, wave 2:** orchestrate `--max-parallel` (width guard) + `--retries` + run results persisted to data/orchestrate.semdb; schedule per-job `last_exit` in list + cooperative tick lock (daemon+manual double-fire closed); supervise `logs <svc> --tail`, crash-loop backoff (15s→480s cap), restarts column, tighter scheduler needle; skills `validate` + tolerant discovery (one bad SKILL.md no longer hides the rest); context stat freshness (age + STALE ≥90d); mcp stdio read timeout + stderr capture (silent hangs now diagnosable) + HTTP bearer auth via `--auth-env`; notify `--click`/`--markdown`/$NTFY_TOKEN auth; search `--pageno`; evals latency n/mean/p50/p95 + diff scores each run once; statusline segments now carry bold labels (🪝 HOOKS: …) via the extension.

**P1 backlog burn-down (from the tool review):** semdb auto-exact <10k rows (HNSW-rebuild-per-query removed), batch embeddings (`fetch_embeddings`, `embed-batch`, rag ingest now ONE POST), `del --prefix`; httpc 64KB header cap + O(n) chunked reads; memory dedup-on-write (cosine ≥0.97 consolidates), explicit ts+hits meta, relevance-based eviction, ts-ordered recent; codegraph `unused` (dead-code candidates); vault `orphans` (+dead links), `tags`/`search --tag`, rename now rewrites [[x|alias]]/![[x]]/[[x#anchor]].

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
