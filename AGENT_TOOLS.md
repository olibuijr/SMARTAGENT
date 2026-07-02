# SMARTAGENT tools

You are running inside the SMARTAGENT project with these custom tools available
(each is a pure-Rust binary; call the tool, don't reimplement it). Use them
instead of guessing or shelling out manually.

**Token discipline:** most tools take scope/limit flags — use them. Default to the cheapest form that answers the question (count/ids/files before pulling rows; snippets/ids-only before full text; quiet before a page dump), then widen only if needed.

- **semdb** — vector store. embed, search (`idsOnly`, `metaChars` to shrink), get, del, count (`prefix`), ids (`prefix`), stats.
- **memory** — 3-tier memory. remember, update (correct by id — prefer over remembering a contradiction), recall (`scope` to one tier), recent, forget, promote, stats. Past-session intents auto-recalled at launch.
- **codegraph** — Rust code graph: index, defs, callers, refs, search, stats.
- **codeindex** — fast code search. `mode`: count (totals only) / files (names) / lines (default, capped at `max`=50). Use count/files first to gauge breadth cheaply.
- **vault** — markdown brain: new, read, append, rm, mv (rewrites [[links]]), list, links, graph, search.
- **skills** — SKILL.md loader: list, show, search, match (score skills against a whole prompt — use to pick the right skill for a task/step).
- **schedule** — durable scheduler. add (`notify` message; `cron` recurring OR `at` YYYY-MM-DDTHH:MM one-shot), pause, resume, list, next, rm, tick. A supervised daemon fires jobs. Arbitrary shell is admin-only.
- **search** — SearXNG web search: query (`timeRange` day|week|month|year, `site` domain, default k=5), health. Results fenced UNTRUSTED.
- **notify** — push notifications (ntfy): send.
- **secrets** — policy-gated store: set, get (as 'pi'), list, audit. Deny by default; ChaCha20-Poly1305 at rest; grants are admin-only. Never read secrets another way.
- **browser** — real Chrome over CDP: open, click, type (`enter` submits), wait (for selector), scroll, attr (read text/value/attribute — cheap, no snapshot), back, probe. `quiet` returns status only (no page dump — use for intermediate steps); `maxText`/`maxLinks` shrink snapshots. Content fenced UNTRUSTED.
- **supervise** — manage background services (scheduler, chromium): status, up, down, restart. Run 'status' first when browser/search/schedule fail.
- **orchestrate** — fan out N headless-pi subagents: run, list, out (collect a run's subagent output).
- **mcp** — connect to any MCP server (stdio or HTTP): tools, call.
- **sandbox** — isolated shell exec (secrets masked): run (`tail` keeps last output — right for build logs; default 16KB cap), clean.
- **context** — principal identity/context loader: compose, validate, stat.
- **evals** — trace/score/diff: log, score (`minPass` → error below threshold, `failOnly`), diff, runs.
- **rag** — document RAG: ingest (file or `url`, http), retrieve (`docId` scope, `snippetChars`, `idsOnly`), get (full chunk), delete-doc, stats.
- **tasks** — kanban board (backlog→ready→doing→review→done): board, add/todo, next (pull-based — obey it), move, done, show, list, crit add/check, block/unblock, wip, metrics. WIP limits and criteria-gated done are ENFORCED; read the kanban skill (skills show kanban) for methodology. Track ALL multi-step work here — no invisible WIP.
- **workflow** — process engine: list, show, start (`task_id` links a board task), step, advance (evidence REQUIRED — describe what you verified), runs, abort. Built-ins: task-run (observe→plan→execute→verify→learn, a skill per step), triage, retro. Use task-run for every non-trivial board task.

_(voice — STT/TTS — built but disabled: no titan speech server. See extensions/disabled/.)_

Prefer these tools for their domains. Endpoints/config come from
config/smartagent.conf; all persistent data lives in semdb tables.
