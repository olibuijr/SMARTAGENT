# Changelog

## 0.1.0 - Unreleased

- Added `crates/rag`: text and simple PDF-text ingestion, token chunking with byte offsets, external embedding calls via `httpc`, semdb-backed vector+metadata storage, cited retrieval, document deletion, stats, and tests.
- Added `extensions/rag.ts` so pi can ingest and retrieve RAG chunks through the compiled Rust binary.
- Certified RAG with `cargo build --release -p rag`, `cargo test -p rag`, `cargo test --workspace`, codex fusion tester PASS, direct semdb probe, and a `./pi -p` RAG tool run.
