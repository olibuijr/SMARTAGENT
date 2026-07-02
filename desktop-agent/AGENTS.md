# desktop-agent — Agent Instructions

Claude-Desktop-style native GUI (Rust, `eframe`/`egui`) that drives the **real**
`./pi` agent over its RPC mode. The GUI is a renderer + input device; every
capability (tools, memory, skills, sessions) lives in pi + `extensions/` — never
reimplement one here.

## Read first

- **[COMPONENTS.md](./COMPONENTS.md)** — module index + data flow. Keep it
  current: any added/renamed/removed module updates the index in the same commit.
- **[ISA.md](./ISA.md)** — system of record for this app: criteria, decisions,
  verification state.

## Architecture in one paragraph

`main.rs` owns three tab contexts (Chat, Cowork @ repo root; Code @ a selected
`workspaces/` project). Each context lazily spawns `<root>/pi --mode rpc` with
that cwd (`rpc.rs`), reads LF-framed JSONL on background threads, and folds
records into `AgentState` (`agent.rs`) via `conn.rs::pump()` each frame. Views
render state and push `Emit` intents (`emit.rs`); `main.rs` executes them after
layout. Session history is read straight from `.pi/sessions/*.jsonl`
(`sessions.rs`); panel data (kanban strip, git changes) shells out to repo
binaries (`data.rs`).

## Rules (scoped to this directory)

1. **Small focused components.** One job per module, ~350 lines target, 1000
   hard max. Split first, then index the new module in COMPONENTS.md.
2. **Only `eframe` + in-repo path deps** (`httpc` for JSON). No new crates.io
   dependencies.
3. **No mock data.** Every rendered value originates from the pi process, a
   session file, or a repo binary.
4. **UI thread never blocks on child I/O.** Reads happen on the reader/stderr
   threads in `rpc.rs`; anything slower than a few ms (large file loads) gets
   capped or moved off-frame.
5. **Views don't touch `RpcClient`.** They emit intents; `emit.rs` executes.
6. **Answer every `extension_ui_request`.** Unanswered dialogs hang the agent —
   cancel is always a valid reply (`{"cancelled":true}`).
7. Scratch/probes go in the repo's `.scratch/` — never `/tmp`.

## pi RPC protocol (v0.80.3, the contract this GUI speaks)

- Spawn: `<root>/pi --mode rpc` (launcher wires extensions, AGENTS.md/
  AGENT_TOOLS.md injection, secrets token, session dir — parity by construction).
- Framing: one JSON object per LF line, both directions.
- Client → agent: `{type, id?, ...}` commands — `prompt` (add
  `streamingBehavior:"steer"` while a turn streams), `abort`, `new_session`,
  `switch_session {sessionPath}`, `get_state`, `get_session_stats`,
  `get_messages`.
- Agent → client: `response` (id-correlated), events (`agent_start/end`,
  `turn_start/end`, `message_start/update/end` with
  `assistantMessageEvent.{text_delta,thinking_delta,error}`,
  `tool_execution_start/update/end` keyed by `toolCallId`, `queue_update`,
  `compaction_*`, `auto_retry_*`), and `extension_ui_request`
  (`select|confirm|input|editor` block the agent until replied;
  `notify|setStatus|setWidget|setTitle` are fire-and-forget).
- Sessions: `.pi/sessions/*.jsonl` v3 — header line (`cwd`), entries with
  `type` (`message`, `session_info` = display name, `compaction`, …).

## Verify by using

- Build: `cargo build --release -p desktop-agent` (workspace member).
- Tests: `cargo test -p desktop-agent` (jsonw + sessions units).
- Protocol probe (no GUI): pipe `get_state` + a `prompt` line into
  `./pi --mode rpc --no-session` and expect `text_delta`s then `agent_end`.
- Live: run `target/release/desktop-agent`, send a chat prompt, watch a tool
  card go spinner → ✓.

## Gotchas

- A spawned pi **without** `--mode rpc` silently becomes one-shot print mode
  (non-TTY auto-detection). Always pass the flag.
- statusline/other extensions fire ~45 `extension_ui_request`s per session
  start (`setStatus`/`setWidget`) — handle or drop them, never let them queue
  as dialogs.
- `message_end` text is authoritative; deltas can drift — replace, don't append.
- `tool_execution_update.partialResult` is **accumulated**, not a delta.
- Session files can be mid-write (agent streaming); skip malformed trailing
  lines when parsing.
- egui `Button` needs explicit `Sense::click()` when wrapped in `horizontal()`
  responses (tool-card header).
