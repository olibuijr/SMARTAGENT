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

Nineteen tools, each a pure-Rust binary the agent calls through a pi extension:

| Tool | Ported from | What it does |
|------|-------------|--------------|
| `semdb` | vector store | Semantic database: embed, search (HNSW/flat cosine), crash-safe append log |
| `memory` | [mem0](https://github.com/mem0ai/mem0) | 3-tier memory (working/episodic/semantic): remember, recall, recent |
| `codegraph` | [CodeGraph](https://github.com/codegraph-ai/CodeGraph) | Rust code knowledge graph: defs/refs/callers + semantic symbol search |
| `codeindex` | ripgrep | Fast literal/regex code search |
| `vault` | Obsidian | Markdown second brain: notes, wikilinks, backlinks, search |
| `skills` | [Agent Skills](https://github.com/anthropics/skills) | SKILL.md loader: list/show/search |
| `schedule` | [Temporal](https://github.com/temporalio/temporal) | Durable cron scheduler (a supervised daemon fires jobs) |
| `search` | [SearXNG](https://github.com/searxng/searxng) | Web metasearch client |
| `notify` | [ntfy](https://github.com/binwiederhier/ntfy) | Push notifications |
| `secrets` | [Infisical](https://github.com/Infisical/infisical) | Policy-gated, audited secret store (deny by default) |
| `browser` | [browser-use](https://github.com/browser-use/browser-use) | Real Chrome over CDP: open/click/type/back + compact snapshot |
| `orchestrate` | [LangGraph](https://github.com/langchain-ai/langgraph) | Fan out N parallel headless-pi subagents |
| `mcp` | Model Context Protocol | MCP client (stdio + HTTP) |
| `sandbox` | [Daytona](https://github.com/daytonaio/daytona) | Isolated command execution (scrubbed env + namespaces) |
| `context` | TELOS | Principal identity/context loader |
| `evals` | [Langfuse](https://github.com/langfuse/langfuse) | Trace, score, regression-diff |
| `rag` | [RAGFlow](https://github.com/infiniflow/ragflow) | Document ingestion + cited retrieval |
| `voice` | [Pipecat](https://github.com/pipecat-ai/pipecat) | STT/TTS bridge |
| `supervise` | — | Internal process manager for the long-running services |

Plus `httpc` (shared HTTP/1.1 + JSON library) and `session-memory` (a hookless
extension that gives the agent continuity across sessions).

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
inference is external (any OpenAI-compatible `/v1/embeddings` endpoint over plain
HTTP; route TLS through a proxy if needed).

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
sandbox scrubs the parent environment so secrets can't leak into a sandboxed
command. See the **Security posture** section in [AGENTS.md](AGENTS.md).

## Development

- **Pure Rust, `std` only, zero crates.io deps** in every `crates/*` tool. Extensions are thin TS glue.
- **No file over 1000 lines.** Split into scoped modules.
- **All data lives in semdb tables** — no bespoke JSON/JSONL formats for new data.
- **Verify by using** — a capability is done when driven end-to-end from `./pi`, not when it compiles.
- **Borrow, don't invent** — reference repos are shallow-cloned into `.refrepos/` (gitignored) and ported, never wrapped.

The gate (`./build.sh`) enforces the line-count and zero-dep rules, lints
extensions for the silent-registration-failure trap, and smoke-tests that all 19
crate tools register in pi. Cut a release with `./build.sh minor "changelog line"`.

## Project layout

```
crates/         pure-Rust zero-dep tools (one binary each) + httpc library
extensions/     thin pi extensions (TS glue) — one per tool
config/         smartagent.conf (all endpoints)
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
