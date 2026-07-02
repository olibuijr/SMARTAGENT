# CLAUDE.md — SMARTAGENT

> Thin pointer. Read **[AGENTS.md](./AGENTS.md)** first — architecture, crate map, reference repos, conventions, **Gotchas**. Read **[ISA.md](./ISA.md)** — system of record, ISC state.

**Status (v0.1.0+):** 23 crates done, tested, committed — semdb, httpc, memory, codegraph, codeindex, vault, skills, schedule, search, notify, secrets, browser, orchestrate, mcp, sandbox, context, evals, rag, voice, supervise, tasks, workflow, hooks. 20 active pi tools (voice delisted — no titan speech server; hooks/statusline/session-memory are event extensions, not tools). `cargo test --workspace` green (desktop-agent excluded — separate agent's WIP); `./build.sh` gate lints extensions and smoke-tests 20/20 tool registration; no file >1000 lines; zero crates.io deps. Remote: `github.com/olibuijr/SMARTAGENT` (private). 2026-07-02: 12-subagent tool review → all 10 P0 bugs fixed; ranked P1 backlog done (see `Plans/TOOL_REVIEW_2026-07-02.md` for the remaining big-ticket items); kanban `tasks` + evidence-gated `workflow` engine + `skills/Kanban` methodology; Claude-Code-style `hooks` system (config/hooks.conf + hooks.d/, exit-2 blocking, semdb audit); colored+labeled TUI statusline. **Project status SSOT: notes MCP note 39** ("SMARTAGENT — pure-Rust AI agent (dev status)") — *pending sync: notes MCP disconnected this session; update it next session from CHANGELOG Unreleased.*

**Run the agent:** `./pi` from repo root (project-isolated: vendored runtime in `.pi/runtime/`, config in `.pi/agent/`, models via `extensions/akurai-router.ts`; default/test model `codex/gpt-5.4-mini --thinking low`). The launcher auto-loads `extensions/*.ts` and injects `AGENT_TOOLS.md` into the agent's initial context (keep it current with the extensions catalog). Headless: `./pi -p '<prompt>' < /dev/null` (redirect MANDATORY).

**Config:** all endpoints in `config/smartagent.conf` (embeddings, ntfy, searx, browser CDP `:9222`, voice — see the file, never hardcode an IP in docs). Resolve via `semdb::config::Config` (flag → env → file). `workspaces_dir`/`data_dir` resolve to repo root. Nothing hardcoded.

**Fusion workflow (always):** you implement → codex CLI tests (`codex exec --sandbox workspace-write -m gpt-5.4-mini -c model_reasoning_effort=low`) → codex reports PASS/FAIL → you verify the report with a direct spot-check before marking done.

**Memory policy (always):** store durable project facts in the SMARTAGENT-scoped semantic DB for the project root or workspace root (not raw `cwd`) under `workspaces/`. A repo/workspace-local `.smartagent/semdb` at the project root is the default convention. **Do not use Claude/Codex CLI integrated memory or any other global memory** for project facts.

When the user asks for workspace contents, report the folders/projects and files under `workspaces/`, not the repository root.

**Extensions catalog:** the full list of `./pi` tools/extensions and what each does lives in [AGENTS.md](./AGENTS.md) → "Extensions catalog". Keep that table current — update it in the same commit whenever you add, rename, or remove an extension.

Hard rules (repeated here because they bite):

- **Pure Rust, `std` only, zero crates.io deps** in every `crates/*` tool. pi extensions are thin TS glue only — no logic.
- **No file over 1000 lines.** Split into scoped, task-oriented modules before hitting it.
- **Borrow, don't invent.** Reference implementations are cloned in `.refrepos/` — read them, port the concept.
- **Verify by using**: drive each capability end-to-end from pi before claiming done.
- Embeddings/LLM inference is external (OpenAI-compatible HTTP); semdb stores/searches vectors itself.
- **All data in database tables.** `semdb` is the storage engine — persist collections as semdb tables (rows), not bespoke JSON/JSONL. Tables needing semantic recall get a vector column (row embedding); others store rows without one. Add vectors only where meaning-based lookup is needed.
- Worktree agents: branch from a committed base, commit in-worktree; orchestrator merges, re-verifies, pushes.
- Never touch `.refrepos/` contents (read-only references, gitignored).
- Router/model failures: you MAY edit `../AkurAI-Router` and deploy to prod via its `./deploy.sh`, then verify and continue.
- **NEVER use /tmp** (or any path outside the repo) for scratch, probes, test dbs, or config — everything lives inside the repo; use `.scratch/` (gitignored) for throwaways.
