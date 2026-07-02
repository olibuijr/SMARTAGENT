# Tool review — 12 subagent reports, 2026-07-02

One read-only reviewer per tool (or pair). Full reports in session transcript; this file is the actionable distillation. Ranked by value-per-line under the repo constraints (pure Rust std-only, ≤1000-line files, data in semdb tables, no TS logic).

## P0 — real bugs (correctness/security) — **ALL 10 FIXED 2026-07-02** (see CHANGELOG Unreleased)

| Tool | Bug | Fix size |
|---|---|---|
| mcp | JSON injection: `format!` interpolates tool name/args unescaped into JSON-RPC | ~3 lines |
| notify | Header injection: `\r\n` in title/tags/topic not sanitized | ~3 lines |
| codeindex | `-i` + regex broken: line lowercased, pattern not — uppercase patterns never match | ~2 lines |
| schedule | `--at` treated as local in docs but fired in UTC; Feb 30-style dates accepted → one-shot job leaks forever | ~10 lines |
| rag | Re-ingest never deletes old chunks → stale orphans corrupt retrieval | ~2 lines (`delete_doc` first) |
| secrets | `get --as CALLER` is self-asserted — policy gate bypassable by claiming any granted caller | ~30–50 lines |
| semdb | No dimension validation — mixed-dim vectors silently mis-score (zip to shorter) | ~10 lines |
| search | No timeout on query (hung SearXNG blocks 60s); no scheme validation on model-supplied `--instance` (SSRF) | ~10 lines |
| browser | `wait_for_load` missing — fixed sleeps race on slow pages | ~20 lines |
| sandbox | Isolation silently degrades when `unshare` absent; no rlimits (fork-bomb/mem/disk unbounded) | ~15 lines |

## P1 — highest-value missing features (per tool) — **TOP-RANKED ITEMS DONE 2026-07-02** (see CHANGELOG)

Done: semdb auto-exact/batch-embed/del-prefix · memory dedup+ts+relevance-eviction · codegraph unused · vault orphans/tags/robust-rename · search pageno+timeout · notify click/markdown/auth · browser readyState (P0) · orchestrate semdb-results/max-parallel/retries · schedule last-exit/tick-lock · supervise logs/backoff/restarts · skills match/validate/tolerant-discovery · context freshness · mcp stdio-timeout+stderr/bearer-auth · evals latency-aggregates/single-score-diff · **hooks system (new crate)**.

Remaining (explicitly big-ticket/later): rag FlateDecode PDF + hybrid BM25, sandbox default-deny FS, secrets TTL/rotation + Vaultwarden sync + tamper-evident audit, codegraph incremental reindex + graph→semdb migration, mcp named-server registry + SSE, evals LLM-judge + auto trace capture, semdb persisted HNSW (needed only >10k rows now), httpc connection reuse.

- **semdb**: HNSW rebuilt from scratch on EVERY search then discarded — currently slower than brute force, dead weight. Persist the graph (or skip rebuild for small N). Batch put/embed (amortize full-log open). Delete-by-prefix.
- **memory**: dedup-on-write (1-NN check before insert — kills contradiction accumulation); hits/ts metadata + relevance-based eviction instead of FIFO; explicit `ts` field (custom ids break time ordering today).
- **codegraph**: dead-code query (~15 lines from existing edges); incremental reindex by mtime; graph persisted as bespoke JSON not semdb (convention violation).
- **vault**: NO semdb semantic search despite the catalog advertising it (plain keyword walk); orphan/dead-link report (~15 lines); tags; rename misses `[[old|alias]]`/`![[old]]`/`[[old#h]]`.
- **search**: pagination (`pageno`), result caching in a semdb table, language/safesearch, structured image/news fields.
- **notify**: Click/Attach/Actions/Delay/Markdown headers; bearer auth; subscribe/poll.
- **browser**: expose `eval` action (exists internally, ~15 lines); numbered interactive-element snapshot index (browser-use style, kills blind selector guessing); tabs, screenshots, cookies later.
- **orchestrate**: persist run/agent results to a semdb table (convention violation; unblocks async status polling); `--max-parallel` cap (width fork-bomb open); `--retries N`.
- **schedule**: persist last run exit/attempt (data already flows, discarded); pi-prompt jobs (run an agent prompt on schedule); catch-up policy; overlap guard + journal lock (daemon + manual tick can double-fire).
- **supervise**: `logs`/`tail` action; crash-loop backoff in watch (15s restart forever today); user-defined services (registry is a hardcoded 2-vec); tighter `is_alive` needle ("schedule" substring false-positives).
- **skills**: keyword auto-trigger scoring (substring count today); `validate` action for frontmatter; one unreadable SKILL.md fails all discovery.
- **context**: per-file mtime/freshness in stat/validate; selective section injection; multi-root overlays.
- **sandbox**: rlimits (P0); default-deny FS (bind-mount workspace into fresh root instead of masking 2 paths — whole repo + ~/.ssh readable today); grandchildren survive timeout kill without isolation.
- **secrets**: caller auth (P0); TTL/rotation; Vaultwarden sync; plaintext names in index; audit log not tamper-evident.
- **rag**: expose chunk/overlap/kind params in rag.ts (implemented in Rust, dead code); batch embeddings (serial 1-POST-per-chunk today); hybrid BM25+vector later; FlateDecode PDF is the big-ticket gap (most real PDFs fail).
- **evals**: surface stored-but-dead `latency_ms` as count/mean/p50/p95; LLM-judge matcher; auto trace capture from pi sessions; diff recomputes score 4×.
- **mcp**: named-server registry in semdb (+auth headers — authenticated servers unreachable today); stdio read timeout + stderr capture (silent hangs undiagnosable); SSE transport.
- **httpc**: connection reuse; config-driven timeout; O(n²) dechunk on large chunked bodies; header-size cap. TLS stays out (proxy strategy is the right trade).
- **voice**: code complete; re-enable = deploy titan speech server + set voice_*_url + move extension out of disabled/.

## Cross-tool structural themes

1. **"All data in semdb tables" is violated in 4 places**: codegraph (bespoke JSON), vault (files + no vector search), skills/context (per-call FS walks), orchestrate (results only on stdout/dirs).
2. **Process-per-call tax**: every tool reopens/replays its whole store per invocation (semdb full-log read, codegraph reload, vault re-walk). Batch verbs and persisted indexes are the shared cure.
3. **Serial embedding calls** (memory, rag, codegraph, semdb) — a shared batch-embed in `semdb::http` helps all four.
4. **Dead data**: evals latency, supervise start_ticks, schedule exit codes, rag chunk params — implemented, stored, never surfaced. Cheapest wins in the repo.
