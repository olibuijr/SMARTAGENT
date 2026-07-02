# SMARTAGENT tools

You are running inside the SMARTAGENT project with these custom tools available
(each is a pure-Rust binary; call the tool, don't reimplement it). Use them
instead of guessing or shelling out manually.

- **semdb** — semantic database over a vector store. Actions: embed, search, get, del, stats.
- **memory** — persistent 3-tier memory (working / episodic / semantic). Actions: remember, recall, stats. Use to retain and recall facts about the user and work across sessions.
- **codegraph** — Rust code knowledge graph. Actions: index, defs, callers, refs, search (semantic), stats. Use to find where symbols are defined and what calls what.
- **codeindex** — fast code search (ripgrep-style): search (regex/literal), files.
- **vault** — markdown second brain: new, read, append, list, links, graph, search.
- **skills** — Agent Skills (SKILL.md) loader: list, show, search.
- **schedule** — durable cron scheduler: add, list, next, rm, tick.
- **search** — SearXNG web search: query, health.
- **notify** — push notifications (ntfy): send.
- **secrets** — policy-gated secret store: set, get (as a caller), list, audit, policy-allow. Never read secrets any other way.
- **browser** — drive real Chrome over CDP (needs Chrome with --remote-debugging-port): open (returns a compact snapshot), probe.
- **orchestrate** — fan out N parallel headless-pi subagents: run, list.
- **mcp** — connect to any MCP server (stdio or HTTP): tools, call.
- **sandbox** — run a shell command isolated in a scratch workspace (safe for untrusted/destructive commands): run, clean.
- **context** — principal identity/context loader: compose, validate, stat.
- **evals** — trace, score, and regression-diff runs: log, score, diff, runs.
- **rag** — document ingestion + cited retrieval: ingest, retrieve, stats.
- **voice** — speech bridge: stt (WAV→text), tts (text→audio), probe.

Prefer these tools for their domains. Endpoints/config come from
config/smartagent.conf; all persistent data lives in semdb tables.
