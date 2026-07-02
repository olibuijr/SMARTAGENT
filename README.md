# SMARTAGENT

A frontier-level personal AI agent, built lean: [pi](https://github.com/earendil-works/pi)
as the minimal agent spine, every capability a pi extension, and every tool a
**pure-Rust, zero-dependency** binary.

Each capability is ported from the most popular open tool in its category — the
concept is borrowed, the implementation rewritten in `std`-only Rust. No
`npm install`, no `pip install`, no crates.io dependency tree. One `cargo build`
produces the whole fleet, and one launcher (`./pi`) wires it into the agent.

```
 pi  (agent spine — 4 core tools + your extensions)
  │
  ├── extensions/*.ts   thin TS glue (no logic) — registers each tool, shells to a binary
  │
  └── crates/*          pure-Rust, std-only, zero-dep tools (one binary each)
```

## Capabilities

Twenty-three crates — twenty agent tools plus shared libraries — each a
pure-Rust binary the agent calls through a pi extension:

| Tool | Ported from | What it does |
|------|-------------|--------------|
| `semdb` | vector store | Semantic database: embed, search (HNSW/flat cosine), crash-safe append log |
| `memory` | [mem0](https://github.com/mem0ai/mem0) | 3-tier memory (working/episodic/semantic): remember, recall, recent |
| `codegraph` | [CodeGraph](https://github.com/codegraph-ai/CodeGraph) | Rust code knowledge graph: defs/refs/callers + semantic symbol search |
| `codeindex` | ripgrep | Fast literal/regex code search + per-repo workspace project index |
| `vault` | Obsidian | Markdown second brain: notes, wikilinks, backlinks, search |
| `skills` | [Agent Skills](https://github.com/anthropics/skills) | SKILL.md loader: list/show/search/match (prompt-scored routing)/validate |
| `schedule` | [Temporal](https://github.com/temporalio/temporal) | Durable cron scheduler (a supervised daemon fires jobs) |
| `search` | [SearXNG](https://github.com/searxng/searxng) | Web metasearch client |
| `notify` | [ntfy](https://github.com/binwiederhier/ntfy) | Push notifications |
| `secrets` | [Infisical](https://github.com/Infisical/infisical) | Policy-gated, audited secret store (deny by default) |
| `browser` | [browser-use](https://github.com/browser-use/browser-use) | Real Chrome over CDP: open/click/type/back + compact snapshot |
| `orchestrate` | [LangGraph](https://github.com/langchain-ai/langgraph) | Fan out N parallel headless-pi subagents |
| `mcp` | Model Context Protocol | MCP client (stdio + HTTP/HTTPS) |
| `sandbox` | [Daytona](https://github.com/daytonaio/daytona) | Isolated command execution (scrubbed env + namespaces) |
| `context` | TELOS | Principal identity/context loader |
| `evals` | [Langfuse](https://github.com/langfuse/langfuse) | Trace, score, regression-diff |
| `rag` | [RAGFlow](https://github.com/infiniflow/ragflow) | Document ingestion + cited retrieval |
| `voice` | [Pipecat](https://github.com/pipecat-ai/pipecat) | STT/TTS bridge |
| `supervise` | — | Internal process manager for the long-running services |
| `tasks` | kanban | Task board with WIP limits + criteria-gated done, enforced in Rust |
| `workflow` | PAI Algorithm | Markdown-defined process engine: a skill per step, evidence-gated |
| `hooks` | Claude Code hooks | User-configurable lifecycle hooks (block/rewrite/inject), exit-2 contract |

Plus `httpc` (shared HTTP/HTTPS + JSON library), `session-memory` (a hookless
extension that gives the agent continuity across sessions), and `statusline`
(a TUI extension that paints live tool/service health under the input — see
**Statusline** below).

## What's new (post-v0.1.0)

The full detail lives in [`CHANGELOG.md`](CHANGELOG.md); the highlights:

### Features added

- **Full skill library** — every tool and operation is covered by a matching
  skill: `platform` (SMARTAGENT expertise — instant answers about the system's
  architecture, features, and conventions), `code-nav`, `memory-recall`,
  `web-research`, `ops`, `orchestration`, plus `kanban` and `self-test`.
  `skills match '<request>'` routes any prompt to the right one (operating
  loop step 1).
- **Slash commands** — instant TUI commands with zero model round-trip:
  `/board`, `/tasks`, `/skills [query]`, `/status`, `/index [project]`,
  `/projects`, `/runs`, `/audit`, `/memory <query>` (`extensions/commands.ts`
  via pi's `registerCommand`).
- **Workspace project layer** — every repo under `workspaces/` is a first-class
  project: `codeindex projects/index` builds a per-repo file inventory at
  `<repo>/.smartagent/codeindex.semdb`, and a shared `--project <name>`
  (traversal-guarded `semdb::workspace`) scopes tasks (per-repo kanban boards —
  tasks never mix between repos), codegraph (per-repo graphs), memory, rag,
  and workflow run state to `<repo>/.smartagent/`.
- **Enforced operating loop** — the agent's order of work (skills match → tasks
  pull → workflow → investigate → execute → verify → close) lives in
  `AGENT_TOOLS.md` and is backed by hooks: file edits are BLOCKED while nothing
  is in `doing` (block message = the unlock commands), agent start injects live
  board/workflow/index state, agent end audits the board snapshot.
- **Task management + process engine** — `tasks` (kanban board: backlog→ready→
  doing→review→done, WIP limits and criteria-gated done enforced in Rust,
  pull-based `next`, flow metrics) and `workflow` (markdown-defined phase loops
  where each step names the skill to use and advancing requires evidence —
  the PAI Algorithm pattern as data). The `skills/Kanban` skill ships the
  methodology plus runnable task-run / triage / retro workflows, and
  `skills match` picks the right skill for a prompt.
- **Hooks system** — Claude-Code-style lifecycle hooks in pure Rust:
  `config/hooks.conf` declares event/matcher/command; hook scripts get the
  payload as JSON on stdin and can block (exit 2), rewrite tool input, or
  inject context. Bridged to pi tool_call / user_prompt / session_start / stop;
  every firing audited to a semdb table. Seeded hooks: destructive-command
  guard, the kanban edit gate, session state brief, stop board audit.

- **TUI statusline widgets** — twelve crates gained a `statusline` verb emitting
  a uniform `level|icon text` protocol (severity decided in Rust). The
  `statusline` extension renders three column-aligned scope-grouped rows below
  the input — workspace `⌂` (code graph, repos indexed + files, tasks board,
  workflow run), data `▦` (memory tiers, rag corpus, next scheduled job, evals,
  orchestrate runs), infra `⛭` (services, sandbox, secrets auth, Chrome,
  SearXNG, hooks) — plus per-tool `⚙ running… → ✓ 142ms` footer activity.
  Healthy segments collapse to `Name ✓`; detail appears on warn/err. Segments
  re-probe after related tool runs and every 30s.
- **Caller-token authentication for secrets** — `secrets get --as C` now
  requires a per-caller token (admin-minted via `issue-token`, 0600 on disk,
  constant-time verify, fail-closed, audited). The `./pi` launcher injects the
  token; the sandbox scrubs it from the environment and masks it on disk.
- **`supervise` process manager** — pure-Rust replacement for per-service
  systemd: spawn/track/health-check/self-heal the scheduler daemon and headless
  Chromium.
- **Session memory** — session intent is captured on shutdown and recalled at
  the next launch, giving the agent cross-session continuity.
- **Tool expansion pass** — browser grew to 8 actions, memory gained
  update/recent/forget/promote, search gained time-range/site filters, schedule
  gained one-shot `--at` + pause/resume, rag gained URL ingest and doc-scoped
  retrieval, and every CLI gained scope/limit/terse flags for token discipline.

### Improvements (ranked P1 backlog, all top items closed)

Batch embeddings everywhere (one POST instead of per-chunk), semdb auto-exact
search below 10k rows + `del --prefix`, memory dedup-on-write with
relevance-based eviction, codegraph dead-code query, vault orphans/tags/robust
rename, orchestrate concurrency cap + retries + persisted results, schedule
last-run exit codes + tick lock, supervise log tailing + crash-loop backoff,
skills validate, context freshness, mcp stdio timeouts + bearer auth, notify
click/markdown/auth, search pagination, evals latency percentiles.

### Fixes (12-subagent review, all 10 P0s closed)

- **Injection**: mcp JSON-RPC injection (tool name/args), notify CR/LF header
  injection.
- **Correctness**: codeindex case-insensitive regex never matched uppercase
  patterns; schedule `--at` fired in UTC while documented as local (now honors
  `utc_offset_minutes` config) and accepted impossible dates (Feb 30) that
  leaked never-firing jobs; rag re-ingest left stale chunks behind; semdb
  silently mis-scored mixed-dimension vectors (per-db dim now enforced).
- **Robustness/security**: search got a 20s timeout and http(s)-only instance
  validation (SSRF guard); sandbox got ulimit resource caps and now warns
  loudly instead of silently downgrading isolation; browser replaced fixed
  sleeps with `document.readyState` polling (~3× faster on fast pages, no race
  on slow ones).

## Statusline

Live health under the input — three scope-grouped, column-aligned rows,
painted green/yellow/red by severity:

```
⌂ 🕸 Code 312sym ✓          · 🗃 Index 27/27 repos, 2.5k files ✓ · ▣ Tasks 1/1 doing · 3 ready ✓ · ▶ Workflow task-run 2/5 ✓
▦ 🧠 Memory w:12 e:340 s:88 ✓ · 📚 Docs 4d/842c ✓               · ⏰ Schedule backup in 13h ✓  · 📊 Evals 9/10 ✓ · 🤖 Agents ✓
⛭ ⛭ Services ✓              · 🧱 Sandbox ✓                      · 🔑 Secrets ✓ · 🌐 Browser ✓ · 🔎 Search: DOWN (searx unreachable) · 🪝 Hooks ✓
```

Each segment is a Rust `<crate> statusline` verb (`ok|…`, `warn|…`, `err|…`) —
the extension only maps level → color, collapses healthy infra segments to
`Name ✓`, and pads columns (wcwidth-aware) so the grid aligns. Red means act
now (service DOWN, token failing verify, SearXNG unreachable); yellow means
attention (stale index, WIP full, blockers, memory tier near its eviction cap).

## Quick start

```sh
# Build the whole fleet (release) + run the full gate (build, test, audits, smoke).
./build.sh

# Run the agent (project-isolated: vendored pi runtime, config, sessions all under .pi/).
./pi

# Headless (the stdin redirect is MANDATORY or pi hangs):
./pi -p 'search the web for Icelandic golf courses' < /dev/null
```

The launcher auto-loads every `extensions/*.ts`, injects the tool catalog
(`AGENT_TOOLS.md`) and recent session memory into the agent's context, and pins
the pi runtime. It never touches the network on a normal launch; update
explicitly with `./pi --self-update` (smoke-tested, auto-rollback).

### Using a tool directly

Every tool is also a standalone binary — useful for scripting and debugging:

```sh
B=target/release/semdb
$B create notes.semdb
$B embed  notes.semdb --id note1 --text "golf swing practice"
$B search notes.semdb --text "sports" --k 5
```

## Configuration

All runtime endpoints live in [`config/smartagent.conf`](config/smartagent.conf)
(embeddings, SearXNG, ntfy, browser CDP, voice). Nothing is hardcoded; every
value resolves **flag → env var → config file** via `semdb::config`. Embeddings
inference is external (any OpenAI-compatible `/v1/embeddings` endpoint over HTTP
or HTTPS; `httpc` uses the system `openssl s_client` helper for TLS and
`SMARTAGENT_HTTPC_CA_FILE` for private roots).

## Long-running services

The agent depends on a couple of background services (a cron **scheduler daemon**
and **headless Chromium** for the browser tool). These are managed by the
internal `supervise` process manager — pure Rust, self-healing, and controllable
by the agent itself:

```sh
supervise status              # state / pid / health of each service
supervise up                  # start them
supervise restart chromium    # restart one
supervise watch               # self-healing loop (restarts dead services every 15s)
```

There is exactly one optional systemd unit, solely to launch the supervisor at
boot. Full details — boot persistence, backups, preflight — in [`ops/README.md`](ops/README.md).

## Security

The agent can invoke every tool and may be prompt-injected via web content, so
guards are deterministic, not prompt-level: untrusted web/search/rag content is
fenced in an explicit envelope; `mcp` and `schedule` cannot smuggle shell
commands; secret grants are admin-only and off the agent's tool surface; the
sandbox scrubs the parent environment, applies ulimit resource caps, and
tmpfs-masks the secret store so secrets can't leak into a sandboxed command;
and secret reads are caller-token-authenticated — a bare `--as pi` claim no
longer works. See the **Security posture** section in [AGENTS.md](AGENTS.md).

## Development

- **Pure Rust, `std` only, zero crates.io deps** in every `crates/*` tool. Extensions are thin TS glue.
- **No file over 1000 lines.** Split into scoped modules.
- **All data lives in semdb tables** — no bespoke JSON/JSONL formats for new data.
- **Verify by using** — a capability is done when driven end-to-end from `./pi`, not when it compiles.
- **Borrow, don't invent** — reference repos are shallow-cloned into `.refrepos/` (gitignored) and ported, never wrapped.

The gate (`./build.sh`) enforces the line-count and zero-dep rules, lints
extensions for the silent-registration-failure trap, and smoke-tests that all 20
active crate tools register in pi. Cut a release with `./build.sh minor "changelog line"`.

## Project layout

```
crates/         pure-Rust zero-dep tools (one binary each) + httpc library
extensions/     thin pi extensions (TS glue) — one per tool
config/         smartagent.conf (all endpoints) + hooks.conf (lifecycle hooks)
hooks.d/        hook scripts (stdin JSON in, exit 2 blocks)
skills/         agent skills (SKILL.md) incl. Kanban methodology + workflows
workflows/      standalone workflow definitions for the workflow engine
ops/            supervisor boot unit, backup + preflight scripts, ops docs
pi              project launcher (self-contained pi runtime under .pi/)
build.sh        build + test + audits + smoke gate; release versioning
AGENTS.md       architecture, crate map, conventions, security posture
AGENT_TOOLS.md  the tool catalog injected into the agent's context
ISA.md          Ideal State — the verifiable criteria driving the build
```

Not tracked (see [`.gitignore`](.gitignore)): `.pi/` (runtime + credentials),
`data/` (memory, secrets, semdb tables), `workspaces/`, `.refrepos/`, `.scratch/`.

## License

MIT
