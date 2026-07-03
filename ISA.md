---
project: SMARTAGENT
task: Rebuild best-of-breed agent stack as pi extensions + pure-Rust 0-dep tools
effort: E5
phase: build
progress: 133/138
mode: build
started: 2026-07-02T01:10:00Z
updated: 2026-07-02T19:30:00Z
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

## Current status — 2026-07-02

Recent board progress closed the local hygiene/design loop:

- T-72: root `.gitignore` now excludes generated/local agent state, including `workspaces/`, `.pi/`, `data/`, `.scratch/`, `.refrepos/`, `.agents/`, `.claude/`, and `.codex/`.
- T-8: `MULTIROLE.md` documents the GOAGENT-style multirole handoff model for `./pi`: board-as-handoff, Planner/Builder/Tester/Researcher/Ops/Coordinator roles, tool/skill mapping, TDD/dev-team/ops standards, and gateway multi-agent implementation plan.
- T-10: stale running workflows for already-done tasks were aborted; active in-progress work owned by another agent (T-71/W-13) was left untouched.
- T-11: `build.sh` gained `lint_pi_imports` plus `./build.sh import-lint-test`, covering multiline runtime imports, oddly spaced runtime imports, and false-positive protection for multiline `import type`.
- T-12: `build.sh` tool registration smoke is deterministic: it extracts `pi.registerTool` names from `extensions/*.ts` and has `./build.sh tools-smoke-test` coverage instead of asking an LLM for a prose tool list.
- T-13: README now distinguishes safe offline `./pi` / headless launches from explicit networked, mutating `./pi --self-update` that may replace `.pi/runtime/`.
- T-14: `/status` and the live statusline share `extensions/lib/statusline-common.ts` for status probes and `level|text` parsing, reducing drift between command and widget output.

Reset-continuation rule: after a session reset, “continue with all tasks” means route through `skills match`, inspect the board, do **not** touch tasks already in `doing` for another agent, pull/refine the highest-priority available work, run `task-run` for non-trivial tasks, verify criteria with real probes, close tasks properly, and keep memory/docs current.

Current changed files from this continuity pass and recent completed work: `.gitignore`, `MULTIROLE.md`, `build.sh`, `README.md`, `ISA.md`, `extensions/commands.ts`, `extensions/statusline.ts`, and `extensions/lib/statusline-common.ts`. Other visible edits in `desktop-agent/*` or `crates/gateway/src/beat.rs` may be from another agent and should not be overwritten casually.

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
- [x] ISC-17: codegraph: indexes supported-language repos (Rust, Python, JS/TS, Go, Java/Groovy, C/C++/CUDA/Metal, C#, Kotlin, Scala, Ruby, PHP, Swift) → symbols/edges queryable (`defs`, `refs`, `callers`, `impls`, `path`, `unused`)
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
- [x] ISC-73: every active tool covered by a matching skill (coverage table; `skills validate` passes for the live skill set)
- [x] ISC-74: `platform` skill answers SMARTAGENT×pi questions, rank-1 on "what can this platform do" etc.
- [x] ISC-75: new domain skills (code-nav, memory-recall, web-research, ops, orchestration) rank-1 on realistic trigger queries
- [x] ISC-76: skills discovery skip-list — `.refrepos`/dot-dirs/target/node_modules/workspaces never surface as skills
- [x] ISC-77: operating loop routes skills (step 1 incl. platform); triage/retro steps name the kanban skill
- [x] ISC-78: slash commands registered via pi.registerCommand — /board /tasks /skills /status /index /projects /runs /audit /memory, verified emitting command messages
- [x] ISC-79: live finale — pi creates workspaces/hello-loop through the loop, serves localhost:8377, marker text confirmed by external curl
- [x] ISC-80: Anti: no skill body documents a command that doesn't exist (agents verified against binaries/extensions)

### sa-browser — DOM snapshots + high-DPI ASCII page rendering in a TUI split pane (2026-07-02)

- [x] ISC-81: `crates/sa-browser` exists, pure-Rust std-only, builds in the workspace release profile
- [x] ISC-82: inflate module decodes zlib/DEFLATE streams (stored, fixed-Huffman, dynamic-Huffman) with unit tests
- [x] ISC-83: png module decodes Chrome screenshot PNGs (8-bit RGBA/RGB/gray, all 5 filters) with unit tests
- [x] ISC-84: art module renders an RGBA buffer to half-block (▀) truecolor ANSI at requested cols×rows with unit test on known pixels
- [x] ISC-85: browser::cdp gains `screenshot_png()` (Page.captureScreenshot → decoded bytes) reusable by sa-browser
- [x] ISC-86: browser::cdp gains `page_status()` returning url + title + readyState for the address bar
- [x] ISC-87: `sa-browser pane --cols N --rows M` emits one status header line then ANSI art lines against live Chrome
- [x] ISC-88: `sa-browser open <url>` navigates and returns the compact DOM snapshot text
- [x] ISC-89: `sa-browser snapshot` returns the DOM snapshot of the current page without navigating
- [x] ISC-90: `sa-browser probe` reports DevTools reachability as one line
- [x] ISC-91: `extensions/sa-browser.ts` registers the sa-browser tool; `./build.sh` gate passes at 21/21 active tools
- [x] ISC-92: activation shows a right-side overlay pane — width 50%, anchor top-right, nonCapturing
- [x] ISC-93: pane top shows an address bar with current URL and page title
- [x] ISC-94: pane shows a loading status that transitions loading → complete on navigation
- [x] ISC-95: pane body is the ASCII-art page render fitted to the pane width
- [x] ISC-96: deactivation hides the pane and clears its refresh timer (no leak after deactivate)
- [x] ISC-97: chat editor keeps keyboard focus while the pane is open (nonCapturing verified by typing)
- [x] ISC-98: no new source file exceeds 1000 lines
- [x] ISC-99: zero crates.io deps — sa-browser depends only on workspace crates (browser, httpc, semdb)
- [x] ISC-100: sa-browser unit/integration tests green under `cargo test --workspace`
- [x] ISC-101: AGENTS.md extensions catalog + AGENT_TOOLS.md + CHANGELOG updated in the same commit
- [x] ISC-102: Anti: legacy `browser` tool behavior unchanged — `browser open` still works and still registers in the gate
- [x] ISC-103: Anti: the TS extension contains no pixel/decode logic — pixels→ANSI happens only in Rust
- [x] ISC-104: Anti: no /tmp usage — throwaway artifacts only under `.scratch/`
- [x] ISC-105: migration path documented — plan for porting `browser` features in, retiring it, renaming sa-browser→browser
- [x] ISC-106: live verification — pane activated in a real `./pi` session (or fusion-tested headlessly where TUI probing is impossible)
- [x] ISC-107: DOM snapshot/page text returned to the model is fenced UNTRUSTED
- [x] ISC-108: non-TUI modes (rpc/json/print, headless `-p`) degrade gracefully — no overlay attempted, no crash, gate stays green
- [x] ISC-109: Anti: tool results never contain raw ANSI escape sequences — art goes to the pane, plain text goes to the model
- [x] ISC-110: Chrome-down shows an in-pane error status with supervise hint instead of a dead/crashed pane
- [x] ISC-111: every art line ends with SGR reset — no color bleed into surrounding TUI
- [x] ISC-112: aspect ratio preserved under 1:2 cell geometry; art fits pane width and respects the height cap
- [x] ISC-113: tablet viewport emulation is the pane default (768 CSS px, dsf 2, mobile) with `--device none` escape hatch
- [x] ISC-114: emulated height derives from the pane cell grid so the art fills the pane to the bottom row (rows requested = art rows rendered)
- [x] ISC-115: sextant mode (2×3 px/cell, 2-means fg/bg clustering, correct U+1FB00 mapping incl. ▌▐█ exclusions) is the default; quad and half selectable
- [x] ISC-116: bare-host URLs normalize to https (`visir.is` → `https://visir.is`); explicit schemes and about:/data: untouched

### Self-heal loop — eval failures become board tasks (2026-07-02)

- [x] ISC-117: `evals triage` creates one criteria-gated `eval-fix:` task per failing run (col ready, p2, tags eval-fix)
- [x] ISC-118: re-sweeping with an open eval-fix task for the run creates nothing (dedupe, prefix includes separator)
- [x] ISC-119: a run named after an open board task (`T-n-…`) is skipped — the work is already owned
- [x] ISC-120: re-failure after a COMPLETED fix escalates once to a p1 `eval-fix (recurring)` task tagged escalate; exhausted escalation defers to the orchestrator
- [x] ISC-121: scoring is latest-trace-per-case — a re-logged passing trace flips the case green
- [x] ISC-122: incremental sweep cursor — only traces logged since the last sweep generate tasks; first sweep initializes the cursor and skips history (live: 21 historical traces skipped, second sweep quiet)
- [x] ISC-123: `--all` sweeps full history; `--dry-run` creates nothing and moves no cursor
- [x] ISC-124: generated tasks carry the no-gaming criterion (root-cause fix; no expectation weakening; no hand-logged green traces)
- [x] ISC-125: `hooks.d/eval-triage.sh` fires on the stop event — verified in a live agent session (exit 0, 25ms, in hooks audit)
- [x] ISC-126: Anti: the cursor row and `_trend` row are invisible to `evals` loads and `tasks all()` respectively — no listing pollution
- [x] ISC-127: tasks statusline leads with total open count + last-change trend arrow (▲/▼, persisted across probes)
- [x] ISC-128: Anti: statusline probes at rest do not flatten the trend — flat counts keep the last direction (unit-tested)
- [x] ISC-129: concurrent sweeps are lock-guarded (`.eval-triage.lock`, first-writer-wins, 120s stale expiry, self-cleaning)
- [x] ISC-130: kill switch — `data/eval-triage.off` disables the loop instantly without rebuild/restart

### Codebase review + token efficiency (2026-07-02 ultrathink sweep)

- [x] ISC-131: clippy zero warnings in all non-fleet-owned crates (was 9 workspace-wide; gateway's 1 left for the fleet)
- [x] ISC-132: unbounded tool outputs capped by default — tasks board (15/col +N-more), schedule list (50), vault read (150 lines), mcp call (4000 chars), evals score (20 PASS lines), memory recall (500 chars/row); all with explicit override flags
- [x] ISC-133: secrets audit esc() control-char JSON bug fixed via semdb::json::escape (same class memory fixed before)
- [x] ISC-134: devtools_base + dead supervise::start_ticks deduped/removed; session-memory intent capped at 280 chars at write (board-dump leak into future sessions)
- [x] ISC-135: AGENTS.md extensions catalog → CATALOG.md pointer: injected context −7.3KB (~1.8k tokens/session)
- [x] ISC-136: top-5 tool schema descriptions trimmed 3,428→1,381 chars (~500 tokens/turn, every fleet turn)
- [x] ISC-137: Anti: no behavior removed — every cap has an override flag and the full gate + all crate tests stay green
- [x] ISC-138: deferred findings boarded as fleet tasks T-128..T-131 (pipe-fill deadlock p2; extension/crate dedup sweeps; store-side row caps)

## Test Strategy

| isc | type | check | threshold | tool |
|---|---|---|---|---|
| 1-3,8 | build/audit | cargo build; awk line count; grep deps | exit 0 / zero hits | Bash |
| 4-6 | fs | ls .refrepos; git check-ignore | present/ignored | Bash |
| 9-15 | functional | CLI invocations + kill -9 crash test + brute-force compare | correctness + recall ≥0.9 | Bash |
| 16-31 | functional | per-crate CLI verb probes against live services | expected output shape | Bash/curl |
| 32-33 | e2e | pi headless run (`< /dev/null`, --mode json) driving extensions | task completes | Bash |
| 34-36 | release | cargo test; git tag | pass / tag exists | Bash |
| 81-84,98-100 | build/unit | cargo build/test sa-browser; awk line count; grep deps | exit 0 / zero hits | Bash |
| 85-90 | functional | sa-browser CLI verbs against live Chrome :9222 | expected output shape | Bash |
| 91,101 | gate/docs | ./build.sh 21/21; grep catalog rows | pass / rows present | Bash |
| 92-97,106 | tui | live ./pi session probe (screenshot/typing) or headless fusion fallback | pane behavior observed | Bash/pi |
| 102-104,107 | anti | browser open regression; rg for logic/tmp; UNTRUSTED fence grep | unchanged / zero hits | Bash |

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
| sa-browser-crate (inflate+png+art+cli) | 81-90,98-100 | browser, httpc | yes |
| sa-browser-extension (pane+tool) | 91-97,103,107 | sa-browser-crate | no (after crate) |
| sa-browser-docs+gate | 101,105 | sa-browser-extension | no (same commit) |
| eval-triage (cursor+escalation+hook) | 117-126 | evals, tasks, hooks | no (single author) |
| tasks-statusline-trend | 127,128 | tasks | yes |

## Decisions

- 2026-07-02 (night): self-heal loop shipped with structural guards baked in, not bolted on. SystemsThinking causal-loop pass BEFORE building found the three dominant pathologies: task-spam amplifier (R1), expectation-gaming shortcut (B2 — "Shifting the Burden": editing the eval is always faster than fixing the code), and the retry treadmill (R3). v1 therefore ships dedupe + tracked-run skip + p1-escalation-not-retry + no-gaming criteria IN the same commit as the loop. The live dry-run proved the analysis immediately: a naive full-history sweep would have created 19 tasks from stale append-only traces — fixed with latest-trace-per-case scoring + a sweep cursor (bootstrap skips history). IterativeDepth adversarial lens then found the NEW cheapest gaming path the guards create (hand-logging a fake passing trace) — mitigated in criterion wording, structural fix boarded as T-120 (write-boundary hook, trace provenance, suite-strength watchdog). Known accepted limitation: concurrent stop-hook sweeps have a ms-window dedupe race (worst case one duplicate task, self-corrects next sweep). Statusline trend: total open + direction of last change persisted in a reserved `_trend` row — flat probes keep the last arrow. Cato skipped per standing Claude-family rule; codex fusion not run (offline surface fully unit-tested, 11/11; live surfaces probed directly per the codex-sandbox-network lesson).
- 2026-07-02 (late): headless TUI verification round (tmux 240×67 = 1920×1080-equivalent) surfaced and fixed two latent platform bugs. (1) **agentpanel focus steal**: the GOAGENT sidebar (other session's 7ebc995) auto-opened at session_start via `ctx.ui.custom()` without `nonCapturing: true` (the comment claimed it, the code omitted it) — keyboard focus went to the panel and the chat editor was DEAD in every new TUI session. Fixed + committed with an explicit LOAD-BEARING comment. (2) **httpc::json O(n²) string parse**: `string()` re-validated the entire remaining buffer with `from_utf8` per character; a ~7MB CDP screenshot payload took 47s CPU (pane stuck on ⏳). Root-cause-at-ingestion fix in the shared crate (ASCII-run bulk copy + bounded multibyte validation): pane render 49.9s → 2.4s wall (400× CPU); every JSON consumer (search, mcp, orchestrate, evals, semdb embed) benefits. Regression tests added (4MB-string parse + mixed-run correctness). Also: sa-browser overlay maxHeight 95%→100% (art stopped ~4 rows short of the bottom).
- 2026-07-02: sa-browser VERIFY notes — (1) Advisor (`Inference.ts --mode advisor --auto-state`) cross-contaminated: auto-state loaded a concurrent session's QA-harness ISA and judged that work's gaps (T-16/17/18, commit 1029f2e); its only sa-browser-applicable ask — a real verification surface beyond the gate — already exists (ISA `## Verification` sa-browser block: tmux focus/toggle/chrome-down probes, live Chrome renders, headless-agent run). No empirical conflict → no re-call. (2) Cato skipped per the standing Claude-family-only rule (see 2026-07-02 precedent decisions); the cross-vendor check was the codex fusion tester, whose 3 sandbox-network FAILs were spot-check-refuted by direct probes (protocol worked as designed). (3) codex sandbox cannot reach :9222 — future fusion tests of networked tools should test offline surfaces via codex and leave live probes to the orchestrator.
- 2026-07-02: sa-browser designed (ISC-81..107) — visual browser pane distinct from the text-first `browser` tool. pi has no left/right widget placement (agentpanel precedent), but pi-tui overlays support `width: "50%"`, `anchor: "top-right"`, `maxHeight`, and `nonCapturing: true` — that is the split mechanism: chat keeps the left half and keyboard focus, the pane overlays the right half with address bar + loading status + half-block truecolor page art. All pixel work (zlib inflate, PNG decode, downscale, ANSI emit) is std-only Rust in `crates/sa-browser`; the extension only paints lines. `screenshot_png`/`page_status` land in browser::cdp so sa-browser composes the existing CDP client — this is also the migration seam: browser's click/type/wait/scroll verbs can later be surfaced through sa-browser, browser retired, sa-browser renamed. Tier E4 ISC floor (128) relaxed with math: the feature decomposes into 27 atomic probes; ISA total is 107 — padding to 128 would violate granularity discipline. Delegation floor met per project precedent: Claude-family only (standing rule), codex `exec` fusion tester is the cross-vendor check; orchestrator builds the platform directly per the CORE RULE exemption.
- 2026-07-02: ISA reconciliation review updated stale ISC-1..36 state against the live repo. Verified `SMARTAGENT_STATUS_EMBED_TIMEOUT=3s ./build.sh`: release build/tests/audits passed, 20/20 active pi tools registered, and project-memory status snapshot fell back cleanly while titan embeddings were unreachable. Fixed two drift points found during review: `.pi/agent/settings.json` defaulted to OpenAI `gpt-4o-mini`/medium instead of AkurAI Router `codex/gpt-5.4-mini`/low, and `build.sh` could hang in the optional status embed despite claiming the snapshot never fails the build. Remaining root open items are now explicit: persisted HNSW scale/perf, live embeddings endpoint success, vault semantic search, external voice server re-enable, and a fresh combined memory+search+vault pi E2E.
- 2026-07-02: 12-subagent tool review ran (one reviewer per tool); findings distilled to `Plans/TOOL_REVIEW_2026-07-02.md`. All 10 P0 bugs fixed same day with regression tests (secrets caller-token auth, mcp/notify injection, codeindex -i regex, schedule tz+impossible-dates, rag re-ingest dedup, semdb dim guard, search timeout+SSRF, sandbox rlimits+loud degrade, browser readyState waits). P1 feature backlog remains in the plan file.
- 2026-07-02: Statusline widgets shipped — uniform `level|icon text` protocol on 11 crates; severity classification in Rust, extension only colors (green/yellow/red) and places. Two belowEditor rows (infra ⛭ / data ▦) + per-tool footer activity.

- 2026-07-02: Forge/Cato codex agents skipped per standing user rule (Claude-family teams only) — delegation floor met via 9 research agents + worktree build agents.
- 2026-07-02: SearXNG hosted not rewritten — engine-scraper maintenance is the value, client is the port surface.
- 2026-07-02: TLS moved into `httpc`: HTTPS URLs now use the system `openssl s_client` helper with normal certificate verification and `SMARTAGENT_HTTPC_CA_FILE` for private roots.
- 2026-07-02: E5 ISC floor (≥256) deferred — project ISA starts at 36 spine ISCs; per-crate ISCs grow during waves (refined: will expand as crates land).
- 2026-07-02: TUI statusline shipped — pi natively supports `ctx.ui.setStatus` + `setWidget(placement: belowEditor)`; added `supervise statusline` Rust verb + `extensions/statusline.ts` (per-tool footer statuses from tool_execution events, services widget below input). No new tool registered; logic stays in Rust per constraint.
- 2026-07-02: ISC-26 landed and certified — `crates/rag` ports the RAGFlow ingestion/retrieval slice into std-only Rust, stores chunks as semdb rows, returns `[ID:...]` cited chunks, has `extensions/rag.ts`, passes codex fusion tester, and was driven through `./pi -p`.
- 2026-07-02: Per-project scoping generalized (ISC-46..57) — SystemsThinking pass found three miscoupled stores beyond the tasks ask: codegraph (per-repo data in ONE global slot → silent clobbering on repo switch), memory (policy says per-repo, default was global-only), workflow (runs reference board-scoped T-n ids, must follow the board). Mechanism: shared `semdb::workspace` (resolve/data_path under `<repo>/.smartagent/`), thin `--project` flag per crate, extensions pass `project` through — zero logic in TS. Kept host-global by design: vault, evals, schedule, secrets, session intents. Historical note: codegraph was Rust-only at this point; it is now a std-only multi-language lexical indexer for Rust, Python, JS/TS, Go, Java/Groovy, C/C++/CUDA/Metal, C#, Kotlin, Scala, Ruby, PHP, and Swift. Statusline: 2 crowded lines → 3 scope-grouped lines (⌂ workspace / ▦ data / ⛭ infra); new codeindex segment. Delegation floor: Forge/Cato skipped per standing Claude-family-only rule; codex exec fusion tester (10/10 PASS) is the cross-vendor check.
- 2026-07-02: codeindex gained workspace-project support (ISC-37..45) — projects moved into workspaces/ were invisible (`workspaces` in ALWAYS-skip, no project concept). Design: per-repo structural file inventory in `<repo>/.smartagent/codeindex.semdb` (semdb table per memory policy; no vectors — no meaning-based lookup needed), `--project` scoping for live search, `index --all` restricted to git repos (numeric orchestrate run-dirs and infra dirs excluded). semdb gained `put_many` (single-fsync bulk insert) because per-put `sync_data` would cost one fsync per file row. Fixed latent `positional_dir` bug: flag values (e.g. `-t rs`) could be mistaken for the search dir.

## Changelog

- 2026-07-02 (sa-browser):
  - conjectured: the pi TUI cannot host side-by-side layouts — its widget API only offers aboveEditor/belowEditor placements (agentpanel's GOAGENT port accepted this and flattened its left sidebar into a stacked widget).
  - refuted by: pi-tui's overlay layer (`ctx.ui.custom` with `overlay: true`) — `OverlayOptions` supports percentage width, anchor `top-right`, `maxHeight`, and `nonCapturing: true`, verified live in tmux: a 50%-width right pane rendered while the chat editor kept keyboard focus.
  - learned: persistent split-pane TUI surfaces ARE available to extensions via never-resolving nonCapturing overlays (hold the `done` resolver, control via OverlayHandle); the widget API's placement enum is not the ceiling of pi's layout capability.
  - criterion now: ISC-92 (right-side overlay pane — width 50%, anchor top-right, nonCapturing) — passed with tmux capture evidence.

## Verification

### sa-browser (2026-07-02)

- ISC-81/98/99/100: Bash — `./build.sh` gate PASS: release build+tests (17 sa-browser unit tests), no file >1000 lines, path-deps-only audit ok
- ISC-82: cargo test — stored_roundtrip, fixed_huffman_vector, dynamic_huffman_vector (546-byte BTYPE=2 fixture), adler32_known, corrupt_adler_rejected all ok
- ISC-83: cargo test — decode_rgba_filter0, decode_rgb_sub_filter, decode_gray_up_filter, decode_average_and_paeth_filters, reject_palette_and_interlace, reject_bad_signature all ok
- ISC-84/111/112: cargo test — one_cell_red_over_blue (exact SGR sequence), every_line_ends_with_reset, aspect_fits_width_and_height_cap, never_upscales, alpha_composites_over_white all ok
- ISC-85/87: Bash — `sa-browser pane --cols 60 --rows 16 --url https://example.com` → header + 16 art lines of truecolor ▀ against live Chrome
- ISC-86: Bash — pane header line exactly `https://example.com/\tExample Domain\tcomplete`
- ISC-88/108: pi headless — `./pi -p` agent ran probe→open→activate; open returned Example Domain snapshot; activate answered "needs the interactive TUI" without crashing
- ISC-89: Bash — `sa-browser snapshot` returned TITLE: Example Domain without navigating
- ISC-90: Bash — `probe` → "Chrome DevTools OK at http://127.0.0.1:9222"
- ISC-91: Bash — gate smoke "ok (21/21 extension tools registered in source)"
- ISC-92/93/95/106: tmux TUI probe — real `./pi` session, `/sab https://example.com`: pane occupies right half (cols ~100-200 of 200), bordered address bar `● https://example.com/` + `Example Domain`, 31 ▀ art rows
- ISC-94: tmux captures — loadState machine renders ⏳ while navigating, `● complete` observed post-load; error state `✖` observed in Chrome-down run
- ISC-96: tmux — second `/sab` toggle → 0 ▀ cells on screen; deactivate() clears interval + resolves the overlay promise
- ISC-97: tmux — typed "typing focus test" with pane open; text landed in the left chat editor line, not the pane
- ISC-101: git — catalogs + changelog staged in the same commit as crate+extension (commit hash in git log)
- ISC-102: Bash — `browser open https://example.com` still returns TITLE snapshot; browser remains in gate EXPECTED
- ISC-103/104: codex fusion tester — "no pixel/PNG decoding logic in extension: PASS", "no /tmp usage: PASS"; its 3 network probes failed only inside the codex sandbox and all passed on direct re-run (protocol spot-check)
- ISC-105: Read — migration seam documented in AGENTS.md catalog row, extension header comment, ISA Decision
- ISC-107/109: Read — untrusted() fence + plain() ANSI-strip wrap every model-facing output in extensions/sa-browser.ts
- ISC-110: tmux — `BROWSER_DEVTOOLS=:19999 ./pi` + `/sab`: pane shows `✖ https://example.com` + unreachable error in-body; supervise hint added to async path
- ISC-113/114: Bash — `pane --cols 100 --rows 40 --url visir.is` renders exactly 40 art rows (grid-derived emulated height); sextant tests + unit fit tests green. Known quirk: `--device none` on headless=new Chrome keeps the last override despite clearDeviceMetricsOverride + settle (headless has no real window to fall back to) — tablet default unaffected
- ISC-115: cargo test — sextant_codepoints_known (U+1FB00/02/3B, ▌▐█ exclusions), sextant_cell_splits, quad_cell_left_column_lit, geometry_identical_across_modes; live render shows U+1FB3x glyphs on 39/40 lines
- ISC-116: cargo test — normalize_url tests; live probe `pane --url visir.is` → header `https://www.visir.is/ Forsíða - Vísir`
- ISC-92..97/106/113..116 (headless TUI re-verification, 2026-07-02 late): tmux 240×67 (1920×1080-equivalent) real `./pi` session, `/sab visir.is` → address bar `● https://www.visir.is/` + `Forsíða - Vísir`, 60 sextant-art rows, colored art through row 66/67 (bottom-filled, 1-row slack by design), typed text landed in the chat editor with the pane open, `/sab` toggle closed it (0 art glyphs). Perf: pane call 49.9s → 2.4s after the httpc::json fix

### Self-heal loop (2026-07-02 night)

- ISC-117..124: cargo test — 11/11 evals tests incl. failing_run_creates_one_task_idempotently, refailure_after_done_escalates_once_to_p1, incremental_bootstrap_skips_history_then_catches_new_failures, latest_pass_supersedes_earlier_fail, dry_run_creates_nothing
- ISC-122 (live): first sweep on real data → "cursor initialized — 21 historical trace(s) skipped"; second sweep → "all fresh runs green"; naive dry-run --all had shown a would-be 19-task burst
- ISC-125: Bash — `./pi -p` session end → hooks audit row `{"hook":"eval-triage","event":"stop","exit":0,"ms":25}`; the fleet's own sessions fired it independently minutes later
- ISC-126: unit tests — cursor row invisible to store::load (statusline unchanged live); `_trend` row invisible to all() (trend test asserts empty board)
- ISC-127/128: Bash + unit — statusline reads `ok|▣ 55 open · 0/4 doing · 20 ready` (arrow appears on first change; trend_dir_tracks_last_change_direction covers rise/fall/flat persistence); live on Óli's screen: `▣ 55 open ▼`
- ISC-129/130: unit kill_switch_and_sweep_lock + live probes — lock absent after sweep; `eval-triage.off` → "disabled" message, removed to re-enable
- LOOP CLOSED AUTONOMOUSLY (2026-07-02 ~22:00): triage-created tasks T-100..T-104+ on the board; fleet pulled and completed T-100 (gateway-multi-agent cargo test) and T-101 (orchestrate-list-statusline) with no human involvement — failure → task → fix → done

### Review sweep (2026-07-02 ultrathink)

- ISC-131: Bash — cargo clippy workspace census 9→gateway-only; safe-zone re-run 0 warnings
- ISC-132: Bash — live probes: board prints `… +21 more (tasks list --col backlog)`; all five crates rebuilt green
- ISC-133/134: Read + build — esc() now semdb::json::escape; start_ticks removed (zero call sites verified); intent .slice(0,280)
- ISC-135/136: wc — AGENTS.md 25,977→18,671 B; description extraction 3,428→1,381 chars; import-lint + tools-smoke green
- ISC-137: Bash — full ./build.sh gate PASS post-changes (build+tests+audits+21/21)
- ISC-138: Bash — tasks add → T-128..T-131 on the board
