---
project: SMARTAGENT
task: Rebuild best-of-breed agent stack as pi extensions + pure-Rust 0-dep tools
effort: E5
phase: build
progress: 1/36
mode: build
started: 2026-07-02T01:10:00Z
updated: 2026-07-02T02:44:54Z
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

SMARTAGENT builds clean (`cargo build --release`, workspace) with 15 focused crates, each ported from its category's most popular reference, each ≤1000-line files, each driveable from pi via a thin extension — verified end-to-end, not just compiling.

## Criteria

Foundation:
- [ ] ISC-1: Workspace `cargo build --release` exits 0
- [ ] ISC-2: `rg -c '' crates/ -g '*.rs'`-style line audit shows no .rs file >1000 lines
- [ ] ISC-3: No crate's Cargo.toml has a `[dependencies]` entry on crates.io (grep clean)
- [ ] ISC-4: `.refrepos/` contains shallow clones of all 14 reference repos
- [ ] ISC-5: `.refrepos/` is gitignored (git status clean of it)
- [ ] ISC-6: AGENTS.md and CLAUDE.md exist and name all crates and rules
- [ ] ISC-7: Anti: no TypeScript logic beyond thin pi-extension glue (extensions contain no business logic)
- [ ] ISC-8: Anti: no file in repo imports serde/tokio/reqwest or any external crate

semdb (semantic database — the core):
- [ ] ISC-9: `semdb create <db>` initializes a crash-safe single-file store
- [ ] ISC-10: `semdb put` stores a vector (f32 dims) + JSON-ish metadata, values >4KB survive via overflow handling
- [ ] ISC-11: `semdb search --k 10` returns cosine top-k, correct against brute-force check
- [ ] ISC-12: ANN index (HNSW or IVF-flat) beats brute force >10x at 100k vectors with recall ≥0.9
- [ ] ISC-13: Kill -9 during writes → reopen recovers without corruption (WAL/shadow paging test)
- [ ] ISC-14: `semdb embed` calls external OpenAI-compatible /v1/embeddings over plain HTTP (std::net)
- [ ] ISC-15: Anti: semdb never runs inference in-process

Capability crates (each: binary runs, core verbs work, pi extension drives it):
- [ ] ISC-16: httpc: shared std-only HTTP/1.1 client module used by all networked crates (no duplicated clients)
- [ ] ISC-17: codegraph: indexes a Rust repo → symbols/edges queryable (`defs`, `refs`, `callers`)
- [ ] ISC-18: memory: 3-tier remember/recall (agentmem port) backed by semdb
- [ ] ISC-19: vault: markdown vault CRUD + [[wikilink]] graph + semdb semantic search
- [ ] ISC-20: search: SearXNG client returns parsed results from self-hosted instance
- [ ] ISC-21: browser: AkurAI-AgentBrowser wrapper returns compact snapshot for a URL
- [ ] ISC-22: orchestrate: spawn/route/fan-out N subagents via akurai-router, collect results (LangGraph send/supervisor concepts)
- [ ] ISC-23: schedule: cron-expression parser + durable task file + daemon fires a test job (Temporal durability concept: at-least-once, journal replay)
- [ ] ISC-24: skills: loads SKILL.md (frontmatter + body) per Agent Skills spec, lists/injects on demand
- [ ] ISC-25: sandbox: runs a command isolated (namespaces/landlock + temp worktree), writes can't escape
- [x] ISC-26: rag: text/PDF-text ingestion → chunks → semdb, retrieval returns cited chunks (RAGFlow pipeline concept)
- [ ] ISC-27: evals: JSONL trace log + scoring run + regression diff between two runs (Langfuse concept)
- [ ] ISC-28: secrets: policy-gated get from Vaultwarden/Infisical-pattern store; deny-by-default policy file
- [ ] ISC-29: voice: STT and TTS round-trip via external endpoints (Pipecat pipeline concept)
- [ ] ISC-30: notify: pushes to ntfy topic via HTTP POST
- [ ] ISC-31: Anti: no crate reaches into another crate's data files directly (only via its binary/API)

Integration:
- [ ] ISC-32: pi extension per crate exists under extensions/ and invokes the binary
- [ ] ISC-33: End-to-end: a pi session uses memory + search + vault in one task successfully
- [ ] ISC-34: Each crate has ≥1 integration test under tests/ that passes
- [ ] ISC-35: README.md documents install + one-command demo
- [ ] ISC-36: Initial release tagged v0.1.0 with CHANGELOG.md

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

- 2026-07-02: 12-subagent tool review ran (one reviewer per tool); findings distilled to `Plans/TOOL_REVIEW_2026-07-02.md`. All 10 P0 bugs fixed same day with regression tests (secrets caller-token auth, mcp/notify injection, codeindex -i regex, schedule tz+impossible-dates, rag re-ingest dedup, semdb dim guard, search timeout+SSRF, sandbox rlimits+loud degrade, browser readyState waits). P1 feature backlog remains in the plan file.
- 2026-07-02: Statusline widgets shipped — uniform `level|icon text` protocol on 11 crates; severity classification in Rust, extension only colors (green/yellow/red) and places. Two belowEditor rows (infra ⛭ / data ▦) + per-tool footer activity.

- 2026-07-02: Forge/Cato codex agents skipped per standing user rule (Claude-family teams only) — delegation floor met via 9 research agents + worktree build agents.
- 2026-07-02: SearXNG hosted not rewritten — engine-scraper maintenance is the value, client is the port surface.
- 2026-07-02: TLS out of scope in-tool; https egress routed via akurai-router/localhost proxies.
- 2026-07-02: E5 ISC floor (≥256) deferred — project ISA starts at 36 spine ISCs; per-crate ISCs grow during waves (refined: will expand as crates land).
- 2026-07-02: TUI statusline shipped — pi natively supports `ctx.ui.setStatus` + `setWidget(placement: belowEditor)`; added `supervise statusline` Rust verb + `extensions/statusline.ts` (per-tool footer statuses from tool_execution events, services widget below input). No new tool registered; logic stays in Rust per constraint.
- 2026-07-02: ISC-26 landed and certified — `crates/rag` ports the RAGFlow ingestion/retrieval slice into std-only Rust, stores chunks as semdb rows, returns `[ID:...]` cited chunks, has `extensions/rag.ts`, passes codex fusion tester, and was driven through `./pi -p`.
- 2026-07-02: Per-project scoping generalized (ISC-46..57) — SystemsThinking pass found three miscoupled stores beyond the tasks ask: codegraph (per-repo data in ONE global slot → silent clobbering on repo switch), memory (policy says per-repo, default was global-only), workflow (runs reference board-scoped T-n ids, must follow the board). Mechanism: shared `semdb::workspace` (resolve/data_path under `<repo>/.smartagent/`), thin `--project` flag per crate, extensions pass `project` through — zero logic in TS. Kept host-global by design: vault, evals, schedule, secrets, session intents. codegraph stays Rust-only (TS repos index 0 symbols — explanatory note added at index time, multi-language lexing is a separate backlog item). Statusline: 2 crowded lines → 3 scope-grouped lines (⌂ workspace / ▦ data / ⛭ infra); new codeindex segment. Delegation floor: Forge/Cato skipped per standing Claude-family-only rule; codex exec fusion tester (10/10 PASS) is the cross-vendor check.
- 2026-07-02: codeindex gained workspace-project support (ISC-37..45) — projects moved into workspaces/ were invisible (`workspaces` in ALWAYS-skip, no project concept). Design: per-repo structural file inventory in `<repo>/.smartagent/codeindex.semdb` (semdb table per memory policy; no vectors — no meaning-based lookup needed), `--project` scoping for live search, `index --all` restricted to git repos (numeric orchestrate run-dirs and infra dirs excluded). semdb gained `put_many` (single-fsync bulk insert) because per-put `sync_data` would cost one fsync per file row. Fixed latent `positional_dir` bug: flag values (e.g. `-t rs`) could be mistaken for the search dir.
