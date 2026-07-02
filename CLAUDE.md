# CLAUDE.md — SMARTAGENT

> Thin pointer. Read **[AGENTS.md](./AGENTS.md)** first — architecture, crate map, reference repos, conventions. Read **[ISA.md](./ISA.md)** — system of record, ISC state.

Hard rules (repeated here because they bite):

- **Pure Rust, `std` only, zero crates.io deps** in every `crates/*` tool. pi extensions are thin TS glue only — no logic.
- **No file over 1000 lines.** Split into scoped, task-oriented modules before hitting it.
- **Borrow, don't invent.** Reference implementations are cloned in `.refrepos/` — read them, port the concept.
- **Verify by using**: drive each capability end-to-end from pi before claiming done.
- Embeddings/LLM inference is external (OpenAI-compatible HTTP); semdb stores/searches vectors itself.
- Worktree agents: branch from a committed base, commit in-worktree; orchestrator merges, re-verifies, pushes.
- Never touch `.refrepos/` contents (read-only references, gitignored).
