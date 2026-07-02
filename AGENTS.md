# SMARTAGENT — Agent Instructions

Frontier-level personal AI agent, rebuilt lean: **pi as the spine, every capability a pi extension, every tool a pure-Rust zero-dependency binary.**

## Fusion workflow (mandatory)

Every feature follows the same loop: **Claude implements → codex CLI tests → codex reports → Claude verifies the report against reality.** The tester is always non-interactive codex: `codex exec --sandbox workspace-write -m gpt-5.4-mini -c model_reasoning_effort=low "<test instructions, PASS/FAIL per check, one-line verdict>" < /dev/null`. **The `< /dev/null` is MANDATORY — without it codex exec blocks forever on "Reading additional input from stdin..." (same gotcha as `./pi -p`).** Claude never self-certifies: a crate is done only after the codex verdict is PASS *and* Claude spot-checks the evidence (verdicts can hallucinate — verify at least one claim with a direct probe).

## Memory policy (mandatory)

**Always store durable project facts in the SMARTAGENT-scoped semantic DB** for the project root or workspace root (not raw `cwd`) under `workspaces/`. A repo/workspace-local `.smartagent/semdb` at the project root is the default convention. **Do not rely on Claude/Codex CLI integrated memory or any other global agent memory** for project facts. Keep memory local to SMARTAGENT and its workspaces so facts do not bleed across repositories.

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
   browser/      wrap AkurAI-AgentBrowser snapshots (Browser Use role)
   orchestrate/  subagent spawn/route/fan-out via akurai-router (LangGraph role);
                 subagent workspaces live in ./workspaces/ (project dir, gitignored),
                 launched from the project cwd
   mcp/          MCP client bridge — connect pi to any MCP server (stdio + HTTP)
   context/      principal identity/context loader (TELOS pattern) injected per session
   schedule/     cron + wake-ups + durable background tasks
   skills/       SKILL.md loader (Agent Skills open standard)
   sandbox/      isolated exec — worktrees + landlock/namespaces (Daytona role)
   rag/          document ingestion: pdf/text → chunks → semdb
   evals/        signal capture, trace log, regression evals
   secrets/      policy-gated credential access (Vaultwarden client)
   voice/        STT/TTS bridge (titan-hosted models)
   notify/       notifications (ntfy-style push)
```

Embeddings inference is **external**: titan embeddinggemma at `http://100.88.0.2:8081` (akurai mesh; OpenAI-compatible, verified 2026-07-02); semdb stores and searches vectors itself — plain-HTTP client written on `std::net`, no TLS dep (route TLS through akurai-router if needed).

## Reference repos (do not invent — mimic)

Popularity winners researched 2026-07-02; we clone the *pattern*, in Rust:

| Capability | Reference | We build |
|---|---|---|
| Agent core | [earendil-works/pi](https://github.com/earendil-works/pi) | use as-is |
| Code graph | [codegraph-ai/CodeGraph](https://github.com/codegraph-ai/CodeGraph) | `crates/codegraph` |
| Memory | [mem0ai/mem0](https://github.com/mem0ai/mem0) | `crates/memory` |
| Second brain | Obsidian (md vault pattern) | `crates/vault` |
| Agent browser | [browser-use/browser-use](https://github.com/browser-use/browser-use) | `crates/browser` (wraps AkurAI-AgentBrowser) |
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

## Conventions

- Build: `cargo build --release` at workspace root; each crate also builds standalone.
- Tests: unit tests in-module, integration under `crates/<name>/tests/`. Gates before merge.
- Versioning: workspace semver lockstep, `CHANGELOG.md` per release (myagents pattern).
- Existing Rust to harvest: AkurAI-Framework (B+tree, HTTP), AkurAI-AgentBrowser, agentmem, AkurAI-Router, akurai-passvault.
- ISA.md in this repo is the system of record — read it before any work; update ISCs as you land them.
