---
project: SMARTAGENT
task: Rebuild best-of-breed agent stack as pi extensions + pure-Rust 0-dep tools
effort: E5
phase: build
progress: 75/80
mode: build
started: 2026-07-02T01:10:00Z
updated: 2026-07-02T17:12:02Z
---

# ISA — SMARTAGENT

## Problem

The most capable agent stacks of 2026 (LangGraph, Mem0, CodeGraph, Browser Use, RAGFlow, Langfuse, …) are fragmented Python/TS ecosystems with heavy dependency trees. Óli wants one coherent frontier-level personal agent: pi as the minimal spine, every capability rebuilt as a focused pure-Rust zero-dependency tool, concepts borrowed from the popularity winner in each category.

## Vision

One `cargo build` produces a fleet of small sharp binaries that give pi memory, code understanding, semantic search, browsing, scheduling, skills, sandboxing, ingestion, evals, secrets, voice, and notifications — with nothing to `npm install` or `pip install`, every file readable in one sitting, and the whole system verifiable end-to-end from a single pi session.

## Out of Scope

Rewriting pi itself; rewriting SearXNG's engine scrapers (host it, client it); TLS implementation in-tool (route via akurai-router/localhost); GUI/dashboard; multi-tenant packaging (later AkurAI extraction); training or hosting models (inference is external OpenAI-compatible HTTP).

## Principles

- Borrow, don't invent: every design decision traceable to a reference repo in `.refrepos/`.
- Smallest correct implementation; debloat after correctness.
- A capability exists only when pi can drive it end-to-end.

## Constraints

- Pure Rust, `std` only — zero crates.io dependencies in `crates/*`.
- No source file over 1000 lines; scoped task-oriented modules.
- pi extensions are thin TS glue; all logic lives in Rust binaries.
- semdb is the single storage engine for vectors + metadata (own crash-safe file format; heed AkurAI-Framework overflow-page lesson for >4KB values).
- Worktree agents branch from committed base; orchestrator merges and re-verifies.

## Goal

SMARTAGENT builds clean with 23 focused pure-Rust std-only crates, 20 active pi tools, and the voice crate built but delisted until an external speech server exists. The repo gate verifies release build/test/audits, ≤1000-line tool files, zero crates.io deps under `crates/*`, type-only pi imports, and 20/20 active tool registration. Remaining gaps are explicit backlog items, not stale unchecked launch criteria.

## Criteria

Foundation:
- [x] ISC-1: Workspace `cargo build --release --workspace --exclude desktop-agent` exits 0 (`./build.sh`, 2026-07-02)
- [x] ISC-2: line audit shows no `.rs` file under `crates/` >1000 lines (`./build.sh`, 2026-07-02)
- [x] ISC-3: no tool crate Cargo.toml has a crates.io dependency (path deps only; `./build.sh`, 2026-07-02)
- [x] ISC-4: `.refrepos/` contains shallow clones of all 14 reference repos
- [x] ISC-5: `.refrepos/` is gitignored (verified with `git check-ignore -v`)
- [x] ISC-6: AGENTS.md and CLAUDE.md exist and name crates/rules/current status
- [x] ISC-7: Anti: TS extensions remain thin pi glue; static gate verifies no runtime pi imports and 20/20 tools register
- [x] ISC-8: Anti: tool crates do not import serde/tokio/reqwest or external crates (grep clean except explanatory comments)

semdb (semantic database — the core):
- [x] ISC-9: `semdb create <db>` initializes a crash-safe single-file store
- [x] ISC-10: `semdb put` stores a vector (f32 dims) + JSON-ish metadata, values >4KB survive via overflow handling
- [x] ISC-11: `semdb search --k 10` returns cosine top-k, correct against brute-force check
- [ ] ISC-12: persisted ANN index beats brute force >10x at 100k vectors with recall ≥0.9 (current behavior auto-exacts <10k rows; persisted HNSW remains backlog)
- [x] ISC-13: Kill -9 during writes → reopen recovers without corruption (`kill9_recovery` integration test)
- [ ] ISC-14: `semdb embed` live call to external OpenAI-compatible `/v1/embeddings` succeeds (implementation wired; titan endpoint unreachable in 2026-07-02 review, status snapshot falls back)
- [x] ISC-15: Anti: semdb never runs inference in-process

Capability crates (each: binary runs, core verbs work, pi extension drives it):
- [x] ISC-16: httpc: shared zero-crates HTTP/1.1 client module used by networked crates, with HTTPS transport through system `openssl s_client`
- [x] ISC-17: codegraph: indexes a Rust repo → symbols/edges queryable (`defs`, `refs`, `callers`, `impls`, `path`, `unused`)
- [x] ISC-18: memory: 3-tier remember/recall/update/recent/forget/promote backed by semdb
- [ ] ISC-19: vault: markdown vault CRUD + [[wikilink]] graph + keyword/tag search are shipped; semantic vault search remains open
- [x] ISC-20: search: SearXNG client returns parsed results from a configured instance/mock, with timeout and SSRF guards
- [x] ISC-21: browser: CDP/AkurAI-AgentBrowser wrapper returns compact snapshots and supports open/click/type/back/wait/scroll/attr/probe
- [x] ISC-22: orchestrate: spawn/route/fan-out N subagents via akurai-router, collect persisted run results, cap width, retry
- [x] ISC-23: schedule: cron-expression parser + durable semdb-backed task file + runner tests for due jobs/replay
- [x] ISC-24: skills: loads SKILL.md (frontmatter + body), lists/shows/searches/matches/validates, tolerates unreadable skills
- [x] ISC-25: sandbox: runs commands with scrubbed env, rlimits, namespace isolation when available, sensitive-path masks, and workspace-confined scratch writes; full default-deny FS remains backlog
- [x] ISC-26: rag: text/PDF-text ingestion → chunks → semdb, retrieval returns cited chunks (RAGFlow pipeline concept)
- [x] ISC-27: evals: semdb-backed trace log + scoring run + regression diff between two runs (Langfuse concept)
- [x] ISC-28: secrets: policy-gated local Infisical-pattern store, deny-by-default grants, token-authenticated callers, AEAD at rest; Vaultwarden sync remains backlog
- [ ] ISC-29: voice: STT and TTS round-trip via external endpoints (crate builds/tests against API shapes, but titan speech server is not deployed and extension is disabled)
- [x] ISC-30: notify: pushes to ntfy topic via HTTP POST, with header-injection guard and optional click/markdown/auth headers
- [x] ISC-31: Anti: tool crates use shared library APIs/binaries instead of reaching into other crates' private runtime data

Integration:
- [x] ISC-32: active pi extension per active crate exists under `extensions/`, invokes binaries, and `./build.sh` verified 20/20 active tools register (`voice` remains delisted)
- [ ] ISC-33: End-to-end: a pi session uses memory + search + vault in one task successfully (not re-run in 2026-07-02 ISA review)
- [x] ISC-34: Each crate has unit/integration coverage passing under `./build.sh` (release tests, desktop-agent excluded by design)
- [x] ISC-35: README.md documents install + one-command demo/current capabilities
- [x] ISC-36: Initial release tagged v0.1.0 with CHANGELOG.md; current tag v0.2.0 exists

Workspace project indexing (2026-07-02):
- [x] ISC-37: codeindex `projects` lists direct children of workspaces_dir with repo marker + index status
- [x] ISC-38: codeindex `index <project>` walks the repo (own .gitignore) → rows in `<repo>/.smartagent/codeindex.semdb`
- [x] ISC-39: codeindex `index --all` indexes every repo project, reports per-repo OK/FAIL + summary counts
- [x] ISC-40: codeindex search/files accept `--project <name>` scoped to workspaces_dir/<name>
- [x] ISC-41: `.smartagent` in ALWAYS-skip — re-index never ingests its own index db
- [x] ISC-42: Anti: repo-root search behavior unchanged — root walks still skip workspaces/
- [x] ISC-43: extensions/codeindex.ts exposes projects/index actions + project param; AGENTS.md + AGENT_TOOLS.md catalog rows updated
- [x] ISC-44: cargo test green for codeindex (incl. project roundtrip + traversal-guard tests) and semdb (put_many)
- [x] ISC-45: live `./pi` run indexes workspace repos with per-repo success/fail monitored

Per-project scoping + 3-line statusline (2026-07-02):
- [x] ISC-46: shared `semdb::workspace` module (root/list/resolve/data_path, traversal-guarded) with tests; codeindex delegates to it
- [x] ISC-47: tasks `--project` = per-repo board at `<repo>/.smartagent/tasks.semdb`; boards proven isolated (A's task invisible on B and root)
- [x] ISC-48: codegraph `--project` = per-repo graph for index AND queries (global single-slot clobbering fixed); 0-symbol non-Rust note
- [x] ISC-49: memory `--project` = per-repo store at `.smartagent/memory` (memory-policy alignment); session intents stay global
- [x] ISC-50: rag `--project` = per-repo corpus; friendly no-corpus error instead of ENOENT
- [x] ISC-51: workflow `--project` = per-repo run state (pairs with the repo's tasks board its T-n ids reference)
- [x] ISC-52: statusline widget = 3 scope-grouped lines (⌂ workspace / ▦ data / ⛭ infra), workspace first
- [x] ISC-53: new `codeindex statusline` segment shows repos-indexed/total + files (`ok|🗃 27/27 repos, 2554 files`), warn on unindexed/stale
- [x] ISC-54: Anti: `--project` cannot escape workspaces root (traversal rejected) and never touches host-global stores
- [x] ISC-55: Anti: default (no project) behavior of every tool unchanged; global stores had no repo-scoped data needing migration (verified)
- [x] ISC-56: extensions expose `project` param (tasks/codegraph/memory/rag/workflow); `./build.sh` gate green, 20/20 tools register
- [x] ISC-57: codex fusion tester 10/10 PASS + live `./pi` run 6/6 PASS on project scoping

Enforced operating loop (2026-07-02):
- [x] ISC-58: AGENT_TOOLS.md carries a single 7-step ordered operating loop (skills→tasks→workflow→investigate→execute→verify→close) covering all active tools by situation
- [x] ISC-59: hook `require-doing-task` blocks edit/write while `doing` is empty; block reason contains copy-pasteable unlock commands
- [x] ISC-60: gate honors the target repo's own board for workspaces/<p>/ paths; `.scratch/` and SMARTAGENT_HOOKS_RELAX=1 exempt
- [x] ISC-61: hook `session-brief` injects live board/workflow/index context at agent start
- [x] ISC-62: hook `stop-board-audit` records board snapshot at agent end in the audit trail
- [x] ISC-63: hooks.ts before_agent_start returns the correct BeforeAgentStartResult shape (crash fixed, trivial `./pi -p` run exits 0)
- [x] ISC-64: Anti: read-only work (bash/grep/read, trivial Q&A) passes without a board entry — the gate never blocks non-mutating tools
- [x] ISC-65: live `./pi` run followed the FULL loop unprompted — skills match → tasks todo+criteria → move doing → workflow task-run W-2 evidenced through all 5 steps → crit checks → memory remember → criteria-gated done (23 ordered tool calls)

Engine-drives-pi (2026-07-02):
- [x] ISC-66: `workflow drive <name>` runs a def step-by-step, one fresh headless pi per step, driver-side advancement only
- [x] ISC-67: driver validates each step's final `EVIDENCE:` line via shared `valid_evidence` (trivial/missing → retry → abort)
- [x] ISC-68: step transcripts persisted to `.scratch/workflow-drive/<run>/step*.log`
- [x] ISC-69: Anti: nested drive refused (SMARTAGENT_DRIVE=1 guard) — a driven step cannot drive
- [x] ISC-70: unit-tested end-to-end with fake agents (complete / trivial-evidence-abort / nested-guard)
- [x] ISC-71: LIVE drive of `status-report` completes with real pi steps and recorded evidence
- [x] ISC-72: README reflects 3-line statusline, workspace layer, operating loop, 20-tool gate (was 18/two-row)

Skill coverage + platform expertise + slash commands (2026-07-02, 3× Fable agents):
- [x] ISC-73: every active tool covered by a matching skill (coverage table; skills validate 14/14)
- [x] ISC-74: `platform` skill answers SMARTAGENT×pi questions, rank-1 on "what can this platform do" etc.
- [x] ISC-75: new domain skills (code-nav, memory-recall, web-research, ops, orchestration) rank-1 on realistic trigger queries
- [x] ISC-76: skills discovery skip-list — `.refrepos`/dot-dirs/target/node_modules/workspaces never surface as skills
- [x] ISC-77: operating loop routes skills (step 1 incl. platform); triage/retro steps name the kanban skill
- [x] ISC-78: slash commands registered via pi.registerCommand — /board /tasks /skills /status /index /projects /runs /audit /memory, verified emitting command messages
- [x] ISC-79: live finale — pi creates workspaces/hello-loop through the loop, serves localhost:8377, marker text confirmed by external curl
- [x] ISC-80: Anti: no skill body documents a command that doesn't exist (agents verified against binaries/extensions)

## Test Strategy

| isc | type | check | threshold | tool |
|---|---|---|---|---|
| 1-3,8 | build/audit | cargo build; awk line count; grep deps | exit 0 / zero hits | Bash |
| 4-6 | fs | ls .refrepos; git check-ignore | present/ignored | Bash |
| 9-15 | functional | CLI invocations + kill -9 crash test + brute-force compare | correctness + recall ≥0.9 | Bash |
| 16-31 | functional | per-crate CLI verb probes against live services | expected output shape | Bash/curl |
| 32-33 | e2e | pi headless run (`< /dev/null`, --mode json) driving extensions | task completes | Bash |
| 34-36 | release | cargo test; git tag | pass / tag exists | Bash |

## Features

| name | satisfies | depends_on | parallelizable |
|---|---|---|---|
| base-scaffold | 1,5,6 | — | no (this session) |
| refrepos-clone | 4 | base | yes (background) |
| semdb | 9-15 | base | yes (wave 1) |
| httpc | 16 | base | yes (wave 1) |
| vault | 19 | semdb | wave 2 |
| skills | 24 | base | yes (wave 1) |
| schedule | 23 | base | yes (wave 1) |
| notify | 30 | httpc | wave 2 |
| search | 20 | httpc | wave 2 |
| memory | 18 | semdb | wave 2 |
| codegraph | 17 | semdb | wave 2 |
| rag | 26 | semdb, httpc | wave 2 |
| secrets | 28 | httpc | wave 2 |
| browser | 21 | httpc | wave 2 |
| orchestrate | 22 | httpc | wave 3 |
| sandbox | 25 | base | wave 3 |
| voice | 29 | httpc | wave 3 |
| evals | 27 | base | wave 3 |
| pi-extensions + e2e | 32-36 | all | wave 4 |

## Decisions

- 2026-07-02: ISA reconciliation review updated stale ISC-1..36 state against the live repo. Verified `SMARTAGENT_STATUS_EMBED_TIMEOUT=3s ./build.sh`: release build/tests/audits passed, 20/20 active pi tools registered, and project-memory status snapshot fell back cleanly while titan embeddings were unreachable. Fixed two drift points found during review: `.pi/agent/settings.json` defaulted to OpenAI `gpt-4o-mini`/medium instead of AkurAI Router `codex/gpt-5.4-mini`/low, and `build.sh` could hang in the optional status embed despite claiming the snapshot never fails the build. Remaining root open items are now explicit: persisted HNSW scale/perf, live embeddings endpoint success, vault semantic search, external voice server re-enable, and a fresh combined memory+search+vault pi E2E.
- 2026-07-02: 12-subagent tool review ran (one reviewer per tool); findings distilled to `Plans/TOOL_REVIEW_2026-07-02.md`. All 10 P0 bugs fixed same day with regression tests (secrets caller-token auth, mcp/notify injection, codeindex -i regex, schedule tz+impossible-dates, rag re-ingest dedup, semdb dim guard, search timeout+SSRF, sandbox rlimits+loud degrade, browser readyState waits). P1 feature backlog remains in the plan file.
- 2026-07-02: Statusline widgets shipped — uniform `level|icon text` protocol on 11 crates; severity classification in Rust, extension only colors (green/yellow/red) and places. Two belowEditor rows (infra ⛭ / data ▦) + per-tool footer activity.

- 2026-07-02: Forge/Cato codex agents skipped per standing user rule (Claude-family teams only) — delegation floor met via 9 research agents + worktree build agents.
- 2026-07-02: SearXNG hosted not rewritten — engine-scraper maintenance is the value, client is the port surface.
- 2026-07-02: TLS moved into `httpc`: HTTPS URLs now use the system `openssl s_client` helper with normal certificate verification and `SMARTAGENT_HTTPC_CA_FILE` for private roots.
- 2026-07-02: E5 ISC floor (≥256) deferred — project ISA starts at 36 spine ISCs; per-crate ISCs grow during waves (refined: will expand as crates land).
- 2026-07-02: TUI statusline shipped — pi natively supports `ctx.ui.setStatus` + `setWidget(placement: belowEditor)`; added `supervise statusline` Rust verb + `extensions/statusline.ts` (per-tool footer statuses from tool_execution events, services widget below input). No new tool registered; logic stays in Rust per constraint.
- 2026-07-02: ISC-26 landed and certified — `crates/rag` ports the RAGFlow ingestion/retrieval slice into std-only Rust, stores chunks as semdb rows, returns `[ID:...]` cited chunks, has `extensions/rag.ts`, passes codex fusion tester, and was driven through `./pi -p`.
- 2026-07-02: Per-project scoping generalized (ISC-46..57) — SystemsThinking pass found three miscoupled stores beyond the tasks ask: codegraph (per-repo data in ONE global slot → silent clobbering on repo switch), memory (policy says per-repo, default was global-only), workflow (runs reference board-scoped T-n ids, must follow the board). Mechanism: shared `semdb::workspace` (resolve/data_path under `<repo>/.smartagent/`), thin `--project` flag per crate, extensions pass `project` through — zero logic in TS. Kept host-global by design: vault, evals, schedule, secrets, session intents. codegraph stays Rust-only (TS repos index 0 symbols — explanatory note added at index time, multi-language lexing is a separate backlog item). Statusline: 2 crowded lines → 3 scope-grouped lines (⌂ workspace / ▦ data / ⛭ infra); new codeindex segment. Delegation floor: Forge/Cato skipped per standing Claude-family-only rule; codex exec fusion tester (10/10 PASS) is the cross-vendor check.
- 2026-07-02: codeindex gained workspace-project support (ISC-37..45) — projects moved into workspaces/ were invisible (`workspaces` in ALWAYS-skip, no project concept). Design: per-repo structural file inventory in `<repo>/.smartagent/codeindex.semdb` (semdb table per memory policy; no vectors — no meaning-based lookup needed), `--project` scoping for live search, `index --all` restricted to git repos (numeric orchestrate run-dirs and infra dirs excluded). semdb gained `put_many` (single-fsync bulk insert) because per-put `sync_data` would cost one fsync per file row. Fixed latent `positional_dir` bug: flag values (e.g. `-t rs`) could be mistaken for the search dir.
