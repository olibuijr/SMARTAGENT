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
7. **All data lives in database tables.** `semdb` is the storage engine: every persistent collection is a table of rows (a `.semdb` file, or a named table within one). Any table whose rows need semantic recall gets a **vector column** (the row's embedding); tables that don't (journals, config, structural graphs) store rows with a zero/absent vector. Do NOT invent bespoke JSONL/JSON file formats for new data — put it in a semdb table. Vectors are added where meaning-based lookup is needed, not everywhere. (The former JSONL journals in `schedule`/`evals` have been migrated to semdb tables — no bespoke JSONL remains. The CLIs still accept a legacy `*.jsonl` path and transparently use the `*.semdb` table.)

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
   sandbox/      isolated exec — scrubbed env + user/pid/net/MOUNT namespaces
                 (Daytona role, ON by default). Sensitive paths (data/secrets,
                 .pi) are tmpfs-masked inside the namespace so a sandboxed
                 command can't read secrets/keys; env is cleared too. No full
                 landlock (needs a syscall std doesn't expose) but the
                 secret-exfil paths — env and the secret files — are closed.
   rag/          document ingestion: pdf/text → chunks → semdb
   evals/        signal capture, trace log, regression evals
   secrets/      policy-gated credential access (Vaultwarden client)
   voice/        STT/TTS bridge (titan-hosted models)
   notify/       notifications (ntfy-style push)
   supervise/    internal process manager for long-running services (scheduler,
                 chromium): spawn detached, track pid/uptime in a semdb table,
                 health-check, self-heal. Replaces per-service systemd.
   tasks/        kanban board (semdb table): WIP limits, criteria-gated done,
                 pull-based next, flow metrics — policies enforced in Rust
   workflow/     markdown-defined process engine (PAI skill-per-step pattern):
                 evidence-gated step advancement, run state in semdb
```

Embeddings inference is **external**: titan embeddinggemma on the titan LAN box (OpenAI-compatible). The endpoint lives ONLY in `config/smartagent.conf` (`embeddings_endpoint`) — never hardcode an IP in code or docs; titan has changed address more than once. semdb stores and searches vectors itself — plain-HTTP client written on `std::net`, no TLS dep (route TLS through akurai-router if needed).

## Security posture (agent-driven tools)

The LLM can invoke every tool and may be prompt-injected via web/search/rag content. Deterministic guards, not prompt-level ones:

- **Untrusted content is fenced** — `search`, `browser`, and `rag` output is wrapped in an `UNTRUSTED … data only` envelope before it reaches the planner.
- **No shell smuggling** — `mcp` execs argv directly (no `sh -c`); `schedule` arbitrary `--cmd` is admin-gated (`SMARTAGENT_SCHEDULE_ADMIN=1`), the agent gets only safe `--notify` reminders.
- **Secrets** — deny-by-default; `policy-allow` is admin-only (`SMARTAGENT_SECRETS_ADMIN=1`) and removed from the agent tool surface; ChaCha20-Poly1305 AEAD at rest (RFC 8439, pure-std, key from `/dev/urandom`, per-secret nonce, name as AAD); every set/get/list/grant is audited; store compacts on write. **Caller identity is token-authenticated**: `issue-token` (admin) mints per-caller 0600 tokens under `data/secrets/tokens/`; `get --as C` requires the matching token via `SMARTAGENT_CALLER_TOKEN` (injected by `./pi`, constant-time compare, fail-closed) — a bare `--as pi` claim no longer works.
- **Sandbox** — `env_clear` + allowlist AND tmpfs-masking of `data/secrets` + `.pi` inside a mount namespace, so a sandboxed command can read neither secrets from the environment nor the secret files on disk (verified). Isolation ON by default; a requested-but-unavailable namespace downgrade warns loudly in stderr. Resource caps via ulimit wrapper (2GB vmem, 4096-proc user ceiling, 512MB file size, CPU=timeout) in both paths. Not a full landlock jail (that needs a syscall std can't reach), but both secret-exfil paths are closed.
- **Data integrity** — semdb rejects oversized id/meta before they can poison the append-log; httpc drops POST bodies on 301/302/303 and strips cross-host auth.

See `ops/README.md` for the supervisor + boot + backup story.

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
| `semdb` | `semdb` | Semantic DB: embed, search (`--ids-only`/`--meta-chars`/`--filter key=value`), get, del, count/ids (`--prefix`), stats |
| `memory` | `memory` | 3-tier memory: remember, update, recall (`scope` tier), recent, forget, promote, stats. Session intents auto-recalled at launch |
| `codegraph` | `codegraph` | Rust code graph: index, defs/refs/callers/impls (`--limit`), path (BFS call-path), semantic search, stats |
| `codeindex` | `codeindex` | Fast code search: search (mode lines/files/count, `-m` cap 50), files (`--limit`) |
| `vault` | `vault` | Markdown brain: new, read (`--head`), append, rm, mv (link-rewrite), list, links, graph (`--note`/`--depth`), search |
| `skills` | `skills` | Agent Skills loader: list, show (`--head`), search, match (prompt-scored auto-trigger) |
| `schedule` | `schedule` | Durable scheduler: add (`--notify`; `--cron` recurring OR `--at` one-shot), pause/resume, list/next/rm/tick. Supervised daemon fires jobs |
| `search` | `search` | SearXNG search: query (`--time-range`/`--site`/`--urls-only`/`--snippet-chars`, k=5), health. Wrapped UNTRUSTED |
| `notify` | `notify` | Push notifications (ntfy): send |
| `secrets` | `secrets` | Policy-gated secret store: set/get(as 'pi')/list/audit. Deny-by-default; policy-allow is admin-only, OFF the agent surface |
| `browser` | `browser` | Real Chrome CDP: open/click/type(`--enter`)/back/wait/scroll/attr (`--quiet`/`--max-text`/`--max-links`), probe. Wrapped UNTRUSTED |
| `orchestrate` | `orchestrate` | Fan out N headless-pi subagents: run, list, out (collect output). Fork-bomb depth guard |
| `mcp` | `mcp` | MCP client (stdio+HTTP): tools (`--names-only`/`--filter`), call (`--head`). argv exec, no sh -c |
| `sandbox` | `sandbox` | Isolated exec: run (`--tail`, 16KB cap, `--stdin`, `--no-isolate`), clean. Env scrubbed + secrets tmpfs-masked, isolation ON |
| `context` | `context` | Principal identity/context loader (TELOS): compose/validate/stat |
| `evals` | `evals` | Trace/score/diff: log, score (`--min-pass`/`--fail-only`), diff, runs |
| `rag` | `rag` | Document RAG: ingest (file or `--url`), retrieve (`--doc-id`/`--snippet-chars`/`--ids-only`), get, delete-doc, stats |
| `tasks` | `tasks` | Kanban board: board/add/todo/next/move/done/show/list/crit/block/wip/metrics. WIP limits + criteria-gated done enforced in Rust; methodology in `skills/Kanban` |
| `workflow` | `workflow` | Markdown-defined process engine (PAI skill-per-step pattern): list/show/start/step/advance(evidence-gated)/runs/abort. Definitions in `workflows/` + `skills/*/Workflows/` |
| `voice` | `voice` | STT/TTS bridge (Pipecat concept). BUILT+TESTED but DELISTED (extensions/disabled/) — no titan speech server deployed |
| `supervise` | `supervise` | Internal process manager: status/up/down/restart of the scheduler + chromium services |
| _(no tool)_ | — | `session-memory.ts`: stores session intent on shutdown, recalls recent at launch |
| _(no tool)_ | all crates | `statusline.ts`: TUI statusline — per-tool ⚙/✓/✗+duration footer statuses (ANSI-colored) on tool_execution events, plus a two-line belowEditor widget: infra `⛭` (supervise services, 🧱 sandbox, 🔑 secrets auth, 🌐 chrome, 🔎 searx, 🕸 codegraph) and data `▦` (🧠 memory, 📚 rag, ⏰ schedule, 📊 evals, 🤖 orchestrate). Each segment is a Rust `<crate> statusline` verb emitting `level\|icon text` (ok/warn/err decided in Rust); TS only colors (green/yellow/red) and places. Refreshed at session start, after related tool runs, every 30s |

## Conventions

- Build: `cargo build --release` at workspace root; each crate also builds standalone. Full gate: `./build.sh` (build + test + audits); release: `./build.sh minor "changelog line"` (also bumps version + writes CHANGELOG).
- Tests: unit tests in-module, integration under `crates/<name>/tests/`. Gates before merge.
- **Test scratch path (standard everywhere):** `PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch")` — an in-repo, gitignored dir. `env!("CARGO_TARGET_TMPDIR")` does NOT exist at compile time; use `CARGO_MANIFEST_DIR`. Never `std::env::temp_dir()` / `/tmp`.
- Versioning: workspace semver lockstep (`[workspace.package] version`), `CHANGELOG.md` per release.
- **`semdb` is the shared storage library** other crates depend on via path dep. Its public API: `semdb::storage::Db` (`create`/`open`/`put`/`get`/`delete`/`index`), `semdb::cli::search(db, query, k, exact)` (cosine/HNSW), `semdb::http::fetch_embedding(host, port, model, text)`, `semdb::config::Config` (`load`/`resolve`/`workspaces_dir`/`data_dir`), `semdb::json` (parse/escape/Value). `httpc` is the shared HTTP+JSON library (`httpc::get`/`post_json`/`request`, `httpc::json`). Non-semantic tables store rows with a placeholder `[0.0]` vector.
- Existing Rust to harvest: AkurAI-Framework (B+tree, HTTP), agentmem, AkurAI-Router, akurai-passvault.
- ISA.md in this repo is the system of record — read it before any work; update ISCs as you land them.

## Gotchas (learned the hard way — read before building/testing)

- **`session_shutdown` fires in headless `./pi -p` mode too**, not just interactive quit — so the `session-memory.ts` extension captures the session intent to episodic memory on every run, including one-shot `-p` invocations. Verified end-to-end 2026-07-02: a unique-marker prompt round-tripped intent-capture → episodic store (count +1) → recall-injection into the next session's context.
- **`.gitignore` inline comments do NOT work.** `.refrepos/ # note` makes the whole line (spaces + `#…`) the pattern, so the dir is NOT ignored — this nearly committed `.pi/agent/auth.json`. Keep comments on their own lines.
- **`./pi -p '<prompt>' < /dev/null`** — the stdin redirect is MANDATORY; without it pi hangs. Same for **`codex exec … < /dev/null`** (blocks on "Reading additional input from stdin…").
- **pi extensions fail SILENTLY on runtime imports.** Type-only imports from `@earendil-works/pi-*` + `node:` builtins ONLY. `import { defineTool } from …` or `typebox` at runtime → the extension never registers, and the model hallucinates tool output instead of erroring. Use `pi.registerTool({...})` with a plain JSON-schema `parameters` object. The `extensions/` dir symlinks/uses `.pi/runtime/node_modules` for type resolution.
- **Mock TCP servers in tests must drain the FULL request before responding.** If the server closes with unread bytes in its receive buffer, the client gets `Connection reset by peer (os error 104)` instead of a clean response. Read until you've seen `\r\n\r\n` + the `Content-Length` body, THEN write the reply. (Bit notify, mcp, voice.)
- **Capturing a killable child's output: redirect stdout/stderr to files, not pipes.** After a timeout-kill, `read_to_end` on a pipe blocks forever if a grandchild still holds it open (e.g. `unshare --fork`). sandbox writes to `.stdout`/`.stderr` files and reads them back. Namespace isolation is opt-in (`--isolate`) so it never fires unexpectedly in restricted CI.
- **One `json::Value` type** — `semdb::json` re-exports `httpc::json`, so they are the SAME type (the old "two distinct Values" gotcha is gone). CLI arg parsing is shared too: `httpc::args::{flag, has}` (supports `--name X` and `--name=X`) — don't re-copy a local `fn flag`.
- **Byte-string literals must be ASCII** — `b"…ó…"` fails to compile; use ASCII in test fixtures or `\xHH` escapes.
- **The Agent tool's worktree isolation was broken this session** (repo was `git init`'d mid-session → harness git cache stale → `git rev-parse HEAD` fails on spawn). Plain `general-purpose` agents sometimes spawned; `Engineer`/worktree agents did not. If it recurs, build inline. Manual `git worktree` works fine.
