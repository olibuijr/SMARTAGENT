# CLAUDE.md — desktop-agent

> Thin pointer. Read **[AGENTS.md](./AGENTS.md)** first — architecture, RPC
> protocol contract, rules, Gotchas. Then **[COMPONENTS.md](./COMPONENTS.md)**
> — module index (keep current in the same commit as any module change) — and
> **[ISA.md](./ISA.md)** — system of record, ISC state.

**What this is:** native Claude-Desktop-clone GUI (Chat / Cowork / Code tabs)
over the repo's real `./pi` agent via `--mode rpc`. GUI renders; pi does.

**Hard rules:** small focused components (~350 lines, split + reindex in
COMPONENTS.md); only `eframe` + in-repo path deps; no mock data; UI thread
never blocks on child I/O; views emit intents, never touch the RPC client;
always answer `extension_ui_request`; scratch in `.scratch/`, never `/tmp`.

**Build/test:** `cargo build --release -p desktop-agent` ·
`cargo test -p desktop-agent` · run `target/release/desktop-agent`.
