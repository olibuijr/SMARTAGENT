# CLAUDE.md — SMARTAGENT

> Thin pointer. Read **[AGENTS.md](./AGENTS.md)** first — architecture, crate map, reference repos, conventions. Read **[ISA.md](./ISA.md)** — system of record, ISC state.

**Fusion workflow (always):** you implement → codex CLI tests (`codex exec --sandbox workspace-write -m gpt-5.4-mini -c model_reasoning_effort=low`) → codex reports PASS/FAIL → you verify the report with a direct spot-check before marking done.

**Memory policy (always):** store durable project facts in the SMARTAGENT-scoped semantic DB for the project root or workspace root (not raw `cwd`) under `workspaces/`. A repo/workspace-local `.smartagent/semdb` at the project root is the default convention. **Do not use Claude/Codex CLI integrated memory or any other global memory** for project facts.

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
