# SMARTAGENT

A frontier-level personal AI agent, rebuilt lean: [pi](https://github.com/earendil-works/pi) as the minimal agent spine, every capability a pi extension, every tool a **pure-Rust, zero-dependency** binary.

Each capability is ported from the most popular tool in its category (researched 2026-07) — concept borrowed, implementation rewritten in `std`-only Rust. No `npm install`, no `pip install`, no crates.io tree. One `cargo build` produces the whole fleet.

## Status

| Crate | Ports | Status |
|---|---|---|
| `semdb` | semantic database (Mem0/vector-store role) | ✅ working, 18 tests |
| `httpc` | shared HTTP/1.1 client lib | 🔨 next |
| `vault` | markdown second brain (Obsidian pattern) | scaffold |
| `skills` | SKILL.md loader (Agent Skills standard) | scaffold |
| `schedule` | durable cron (Temporal concepts) | scaffold |
| wave 2 | memory, codegraph, search, notify, secrets, rag, browser | planned |
| wave 3 | orchestrate, mcp, context, sandbox, voice, evals | planned |

See [AGENTS.md](./AGENTS.md) for the full architecture and reference-repo map, [ISA.md](./ISA.md) for the verifiable criteria driving the build.

## Quick start

```sh
cargo build --release
cargo test --workspace
```

### semdb — semantic database

Crash-safe single-file vector store with CRC-framed log records, brute-force + HNSW cosine search, and external embeddings (any OpenAI-compatible `/v1/embeddings` endpoint over plain HTTP).

```sh
B=target/release/semdb

$B create notes.semdb
$B embed  notes.semdb --id note1 --text "golf swing practice"
$B embed  notes.semdb --id note2 --text "cooking dinner tonight"
$B search notes.semdb --text "sports" --k 5      # semantic search (HNSW)
$B search notes.semdb --vector '0.1,0.2' --exact # raw vector, brute force
$B stats  notes.semdb
$B compact notes.semdb
```

Default endpoint is `100.88.0.2:8081` / model `embeddinggemma`; override with `--endpoint host:port --model name`.

Crash safety: every record is length+CRC32 framed — a torn write from a crash (`kill -9`) is detected and truncated on the next open. Verified by a real killed-writer test in `crates/semdb/tests/`.

## Design rules

- Pure Rust, `std` only — zero external crates, in every tool.
- No source file over 1000 lines; small task-oriented modules.
- Borrow, don't invent: reference repos are shallow-cloned into `.refrepos/` (gitignored) and ported, never wrapped.
- Inference is external (OpenAI-compatible HTTP); tools store, search, schedule, and act.
- Subagent workspaces live in `workspaces/` (gitignored).

## License

MIT
