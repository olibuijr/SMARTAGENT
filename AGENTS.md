# SMARTAGENT — Agent Instructions

Frontier-level personal AI agent, rebuilt lean: **pi as the spine, every capability a pi extension, every tool a pure-Rust zero-dependency binary.**

## Fusion workflow (mandatory)

Every feature follows the same loop: **Claude implements → codex CLI tests → codex reports → Claude verifies the report against reality.** The tester is always non-interactive codex: `codex exec --sandbox workspace-write -m gpt-5.4-mini -c model_reasoning_effort=low "<test instructions, PASS/FAIL per check, one-line verdict>" < /dev/null`. **The `< /dev/null` is MANDATORY — without it codex exec blocks forever on "Reading additional input from stdin..." (same gotcha as `./pi -p`).** Claude never self-certifies: a crate is done only after the codex verdict is PASS *and* Claude spot-checks the evidence (verdicts can hallucinate — verify at least one claim with a direct probe).

## Memory policy (mandatory)

**Always store durable project facts in the SMARTAGENT-scoped semantic DB** for the project root or workspace root (not raw `cwd`) under `workspaces/`. A repo/workspace-local `.smartagent/semdb` at the project root is the default convention. **Do not rely on Claude/Codex CLI integrated memory or any other global agent memory** for project facts. Keep memory local to SMARTAGENT and its workspaces so facts do not bleed across repositories.

When asked for workspace contents, list the folders/projects and files under `workspaces/`, not the repository root.

## The one rule set

1. **Pure Rust, `std` only.** No crates.io dependencies in any tool. No TypeScript logic — pi extensions are thin glue that shell out to Rust binaries.
2. **No file over 1000 lines.** Split before you hit it. Each file has one task-oriented job (imitate AkurAI-Router's scoped-module layout).
3. **Small focused crates.** One capability = one crate under `crates/`. Thin `main.rs`, logic in scoped modules.
4. **Lean.** Smallest correct implementation. No dead code, no speculative features. Port for correctness, then debloat.
5. **Verify by using.** A capability is done when driven end-to-end from pi, not when it compiles.
6. **NEVER use /tmp or any path outside the repo.** Scratch, probes, test databases, config — all inside the repo; throwaways go in `.scratch/` (gitignored).
7. **All data lives in database tables.** `semdb` is the storage engine: every persistent collection is a table of rows (a `.semdb` file, or a named table within one). Any table whose rows need semantic recall gets a **vector column** (the row's embedding); tables that don't (journals, config, structural graphs) store rows with a zero/absent vector. Do NOT invent bespoke JSONL/JSON file formats for new data — put it in a semdb table. Vectors are added where meaning-based lookup is needed, not everywhere. (Existing JSONL journals in `schedule`/`evals` predate this rule and are migration targets.)

## Architecture

```
pi (earendil-works/pi, npm @mariozechner/pi)   ← agent spine, 4 core tools
 └── extensions/          ← thin pi extensions (TS glue only, no logic)
      └── invoke ↓
 crates/                  ← pure-Rust 0-dep tools (one binary each)
   semdb/        semantic database: embeddings store + HNSW/flat cosine search,
                 crash-safe file format (own design — see AkurAI-Framework B+tree,
                 incl. overflow-page lesson for >4KB values)
   codegraph/    code knowledge graph (clone of codegraph-ai/CodeGraph idea):
                 parse → symbols/edges → semdb-backed queries
   memory/       persistent 3-tier agent memory (agentmem pattern, Mem0 role)
   vault/        markdown second-brain read/write/link/search (Obsidian pattern)
   search/       web search client → self-hosted SearXNG instance
   browser/      Browser Use port: pure-Rust CDP client (hand-rolled WebSocket +
                 JSON-RPC over std::net) → navigate a real Chrome, compact snapshot
   orchestrate/  subagent spawn/route/fan-out via akurai-router (LangGraph role);
                 subagent workspaces live in ./workspaces/ (project dir, gitignored),
                 launched from the project cwd
   mcp/          MCP client bridge — connect pi to any MCP server (stdio + HTTP)
   context/      principal identity/context loader (TELOS pattern) injected per session
   schedule/     cron + wake-ups + durable background tasks
   skills/       SKILL.md loader (Agent Skills open standard)
   sandbox/      isolated exec — scrubbed env + opt-in user/pid/net namespaces
                 (Daytona role). NOTE: no landlock/mount-jail yet — a namespaced
                 command still sees the real filesystem; env is cleared so
                 parent secrets don't leak. Isolation is ON by default.
   rag/          document ingestion: pdf/text → chunks → semdb
   evals/        signal capture, trace log, regression evals
   secrets/      policy-gated credential access (Vaultwarden client)
   voice/        STT/TTS bridge (titan-hosted models)
   notify/       notifications (ntfy-style push)
```

Embeddings inference is **external**: titan embeddinggemma at `http://192.168.1.119:8081` (titan LAN; OpenAI-compatible, verified 2026-07-02); semdb stores and searches vectors itself — plain-HTTP client written on `std::net`, no TLS dep (route TLS through akurai-router if needed).

## Reference repos (do not invent — mimic)

Popularity winners researched 2026-07-02; we clone the *pattern*, in Rust:

| Capability | Reference | We build |
|---|---|---|
| Agent core | [earendil-works/pi](https://github.com/earendil-works/pi) | use as-is |
| Code graph | [codegraph-ai/CodeGraph](https://github.com/codegraph-ai/CodeGraph) | `crates/codegraph` |
| Memory | [mem0ai/mem0](https://github.com/mem0ai/mem0) | `crates/memory` |
| Second brain | Obsidian (md vault pattern) | `crates/vault` |
| Agent browser | [browser-use/browser-use](https://github.com/browser-use/browser-use) | `crates/browser` (pure-Rust CDP client — WebSocket+JSON-RPC to real Chrome) |
| Web search | [searxng/searxng](https://github.com/searxng/searxng) | `crates/search` (client; host SearXNG, don't rewrite) |
| Orchestration | [langchain-ai/langgraph](https://github.com/langchain-ai/langgraph) | `crates/orchestrate` |
| Skills | [anthropics/skills](https://github.com/anthropics/skills) (SKILL.md standard) | `crates/skills` |
| Sandbox | [daytonaio/daytona](https://github.com/daytonaio/daytona) | `crates/sandbox` |
| Scheduler | [temporalio/temporal](https://github.com/temporalio/temporal) | `crates/schedule` |
| RAG ingestion | [infiniflow/ragflow](https://github.com/infiniflow/ragflow) | `crates/rag` |
| Evals | [langfuse/langfuse](https://github.com/langfuse/langfuse) | `crates/evals` |
| Secrets | [Infisical/infisical](https://github.com/Infisical/infisical) | `crates/secrets` |
| Voice | [pipecat-ai/pipecat](https://github.com/pipecat-ai/pipecat) (whisper.cpp STT + Kokoro TTS) | `crates/voice` |
| Notifications | [binwiederhier/ntfy](https://github.com/binwiederhier/ntfy) | `crates/notify` |

Reference clones live in `.refrepos/` (shallow, gitignored). **Borrow and port**: read the reference implementation, port the concept into pure Rust — never invent an API the reference doesn't justify, never copy license-incompatible code verbatim wholesale.

## Router failures

Models are served via **AkurAI-Router** (`../AkurAI-Router`). If pi model calls or router behavior fail because of a router bug, you are **allowed to edit `../AkurAI-Router` and deploy to prod with its `./deploy.sh`**. Fix, deploy, verify, then continue.

## Project pi (`./pi`)

- Launch pi ONLY via `./pi` from the repo root — fully self-contained: vendored binary in `.pi/runtime/` (bun package `@earendil-works/pi-coding-agent`), config+router key in `.pi/agent/`, sessions in `.pi/sessions/`. The PATH/global pi is never used. Auto-updates daily (funny progress bar included).
- Models via AkurAI-Router (extension `extensions/akurai-router.ts`). **Default/test model: `codex/gpt-5.4-mini` with `--thinking low`.** Use bigger claude/* models only when explicitly needed.
- Headless testing: `./pi -p '<prompt>' < /dev/null` — the stdin redirect is MANDATORY or pi hangs.
- **Extension pattern (hard rule):** extensions in `extensions/*.ts` are auto-loaded by the launcher. They must use ONLY type-only imports from pi packages plus `node:` builtins — runtime imports (`defineTool`, `typebox`) fail silently and the tool never registers. Use `pi.registerTool({...})` with a plain JSON-schema `parameters` object, and shell out to `target/release/<crate>` via `execFileSync`. No logic in TS.
- Every crate gets an extension; every crate's acceptance test is a natural-language `./pi -p` prompt exercising the tool.

### Extensions catalog (KEEP CURRENT)

Every tool `./pi` exposes, one row per `extensions/*.ts`. **When you add, rename, or remove an extension, update this table in the same commit** — it is the canonical description of the agent's capabilities.

| Tool | Backing crate | What it does |
|------|---------------|--------------|
| `akurai-router` | — (provider) | Registers AkurAI-Router models (claude/*, codex/*) as the pi provider |
| `semdb` | `semdb` | Semantic database: embed/search/get/del/stats over a vector store |
| `memory` | `memory` | 3-tier persistent memory (working/episodic/semantic): remember, recall, stats |
| `codegraph` | `codegraph` | Rust code knowledge graph: index, defs/refs/callers, semantic symbol search |
| `codeindex` | `codeindex` | Fast code search (ripgrep-style): regex/literal search, file listing |
| `vault` | `vault` | Markdown second brain: new/read/append/list/links/graph/search |
| `skills` | `skills` | Agent Skills (SKILL.md) loader: list/show/search |
| `schedule` | `schedule` | Durable cron scheduler: add/list/next/rm/tick |
| `search` | `search` | SearXNG web search: query, health |
| `notify` | `notify` | Push notifications (ntfy): send |
| `secrets` | `secrets` | Policy-gated secret store: set/get(as caller)/list/audit/policy-allow |
| `browser` | `browser` | Real Chrome over CDP (Browser Use port): open (snapshot), probe |
| `orchestrate` | `orchestrate` | Fan out N parallel headless-pi subagents: run, list |
| `mcp` | `mcp` | MCP client (stdio + HTTP): tools, call |
| `sandbox` | `sandbox` | Isolated command execution (Daytona concept): run, clean |
| `context` | `context` | Principal identity/context loader (TELOS): compose/validate/stat |
| `evals` | `evals` | Trace + score + regression diff (Langfuse concept): log/score/diff/runs |
| `rag` | `rag` | Document ingestion + retrieval (RAGFlow concept): ingest, ask, sources |
| `voice` | `voice` | STT/TTS bridge (Pipecat concept): stt, tts, probe |

## Conventions

- Build: `cargo build --release` at workspace root; each crate also builds standalone. Full gate: `./build.sh` (build + test + audits); release: `./build.sh minor "changelog line"` (also bumps version + writes CHANGELOG).
- Tests: unit tests in-module, integration under `crates/<name>/tests/`. Gates before merge.
- **Test scratch path (standard everywhere):** `PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch")` — an in-repo, gitignored dir. `env!("CARGO_TARGET_TMPDIR")` does NOT exist at compile time; use `CARGO_MANIFEST_DIR`. Never `std::env::temp_dir()` / `/tmp`.
- Versioning: workspace semver lockstep (`[workspace.package] version`), `CHANGELOG.md` per release.
- **`semdb` is the shared storage library** other crates depend on via path dep. Its public API: `semdb::storage::Db` (`create`/`open`/`put`/`get`/`delete`/`index`), `semdb::cli::search(db, query, k, exact)` (cosine/HNSW), `semdb::http::fetch_embedding(host, port, model, text)`, `semdb::config::Config` (`load`/`resolve`/`workspaces_dir`/`data_dir`), `semdb::json` (parse/escape/Value). `httpc` is the shared HTTP+JSON library (`httpc::get`/`post_json`/`request`, `httpc::json`). Non-semantic tables store rows with a placeholder `[0.0]` vector.
- Existing Rust to harvest: AkurAI-Framework (B+tree, HTTP), agentmem, AkurAI-Router, akurai-passvault.
- ISA.md in this repo is the system of record — read it before any work; update ISCs as you land them.

## Gotchas (learned the hard way — read before building/testing)

- **`./pi -p '<prompt>' < /dev/null`** — the stdin redirect is MANDATORY; without it pi hangs. Same for **`codex exec … < /dev/null`** (blocks on "Reading additional input from stdin…").
- **pi extensions fail SILENTLY on runtime imports.** Type-only imports from `@earendil-works/pi-*` + `node:` builtins ONLY. `import { defineTool } from …` or `typebox` at runtime → the extension never registers, and the model hallucinates tool output instead of erroring. Use `pi.registerTool({...})` with a plain JSON-schema `parameters` object. The `extensions/` dir symlinks/uses `.pi/runtime/node_modules` for type resolution.
- **Mock TCP servers in tests must drain the FULL request before responding.** If the server closes with unread bytes in its receive buffer, the client gets `Connection reset by peer (os error 104)` instead of a clean response. Read until you've seen `\r\n\r\n` + the `Content-Length` body, THEN write the reply. (Bit notify, mcp, voice.)
- **Capturing a killable child's output: redirect stdout/stderr to files, not pipes.** After a timeout-kill, `read_to_end` on a pipe blocks forever if a grandchild still holds it open (e.g. `unshare --fork`). sandbox writes to `.stdout`/`.stderr` files and reads them back. Namespace isolation is opt-in (`--isolate`) so it never fires unexpectedly in restricted CI.
- **Two `json::Value` types exist** — `semdb::json::Value` and `httpc::json::Value` are distinct; don't mix them across crates (`httpc::get(...).json()` returns httpc's). Pick one module per file.
- **Byte-string literals must be ASCII** — `b"…ó…"` fails to compile; use ASCII in test fixtures or `\xHH` escapes.
- **The Agent tool's worktree isolation was broken this session** (repo was `git init`'d mid-session → harness git cache stale → `git rev-parse HEAD` fails on spawn). Plain `general-purpose` agents sometimes spawned; `Engineer`/worktree agents did not. If it recurs, build inline. Manual `git worktree` works fine.
