---
name: code-nav
description: Navigate and find code with codeindex (fast text search over the repo and workspace projects) and codegraph (Rust symbol graph — where a function or struct is defined, refs, callers, call path, unused). USE WHEN find code, search the codebase, grep for a pattern, where is a function or struct defined, definition, who calls this, callers, references, refs, symbol lookup, call path, unused functions, index a repo, reindex, list workspace projects, explore an unfamiliar repository.
---

# CodeNav — finding your way around code

Two tools, one decision: **codeindex** answers "which files/lines contain X"
(text), **codegraph** answers "how is X wired" (symbols — Rust only). Always
cheaper than reading files top to bottom, always before editing.

## Which tool

| Question | Tool |
|---|---|
| Which files mention `fetch_embedding`? | `codeindex search` |
| Where is `Config::resolve` DEFINED? | `codegraph defs` |
| Who CALLS `guard_depth`? | `codegraph callers` |
| Every REFERENCE to a symbol? | `codegraph refs` |
| How does fn A reach fn B? | `codegraph path` (BFS call-path) |
| Dead code candidates? | `codegraph unused` |
| Fuzzy "something like retry logic"? | `codegraph search` (semantic, needs `--embed` at index time) |
| What repos live under workspaces/? | `codeindex projects` |

## codeindex — text search + project inventory

- `codeindex search <pattern>` — flags: `-i` (case-insensitive), `-e` (regex),
  `-A N`/`-B N` (context), `-t ext` (extension filter), `dir` to scope.
- **Gauge breadth cheaply first**: `mode=count` (totals only) → `mode=files`
  (names) → default lines (capped at `max`=50). Never open with a full line dump.
- `codeindex projects` — repos under `workspaces/` + index status.
- `codeindex index <project>` (or all) — per-repo file inventory at
  `<repo>/.smartagent/codeindex.semdb`. Rebuild after structural changes.

## codegraph — multi-language symbol graph

- `index` first: `--project <name>` for a workspace repo (graph at
  `workspaces/<name>/.smartagent/codegraph.json`), or `index <dir> --out <graph.json>`
  for anything else. Add `--embed` if you'll want semantic `search` later.
- Supported language front-ends are std-only lexical extractors for Rust,
  Python, JavaScript/JSX, TypeScript/TSX, Go, Java/Groovy, C, C++/CUDA/Metal,
  C#, Kotlin, Scala, Ruby, PHP, and Swift.
- Query verbs: `defs`, `refs`, `callers`, `impls`, `path` (two fn names),
  `unused`, `search`, `stats`. All accept `--project <name>` in place of a
  graph path; `--limit` caps output.
- Per-repo graphs never clobber each other — always pass the same `project`
  you indexed with.

## Workspace scoping (the rule that bites)

Root-level `codeindex` walks **deliberately skip `workspaces/`** (and .git,
target, node_modules, .refrepos, .pi, .scratch, .smartagent; .gitignore is
respected). To search a workspace repo you MUST pass `project=<name>` — an
empty result without it proves nothing.

## Gotchas

- **codegraph is a lightweight lexer, not a full parser.** It extracts common
  definitions/calls across supported languages without tree-sitter; for exact
  language-server semantics, fall back to `codeindex search` and file reads.
- `codegraph search` returns nothing useful unless the graph was indexed with
  `--embed` (it's semdb-backed embedding lookup).
- A stale graph lies: after renames/refactors, `codegraph index` again before
  trusting `refs`/`callers` output. The statusline ⌂ row shows staleness.
- Close-out convention: after structural changes to a workspace repo, run
  `codeindex index <project>` and (Rust repos) `codegraph index --project <p>`.
