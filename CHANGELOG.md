# Changelog

## 0.1.0 — 2026-07-02
- Initial release: 19 pure-Rust, std-only, zero-crates.io-dep crates, each a pi extension, ported from the most popular tool in its category.
- Storage: semdb (crash-safe log store + HNSW). All data in semdb tables.
- Crates: semdb, httpc, memory, codegraph, codeindex, vault, skills, schedule, search, notify, secrets, browser (CDP), orchestrate, mcp, sandbox, context, evals, rag, voice.
- Project-isolated ./pi launcher with vendored runtime, config-driven endpoints (config/smartagent.conf), and a tools brief injected into the agent context.
