---
project: desktop-agent
task: Claude Desktop clone GUI at feature parity with ./pi via pi RPC mode
effort: E4
phase: build
progress: 0/132
mode: build
started: 2026-07-02T16:10:00Z
updated: 2026-07-02T16:20:00Z
---

# ISA — desktop-agent

## Problem

`desktop-agent/` is a static egui mockup of Claude Desktop: hardcoded sessions, a canned transcript, fake progress/artifacts, a composer that appends a placeholder reply. It has zero connection to the actual agent. Meanwhile the repo's real agent (`./pi` + 20 Rust tools) is TUI-only. There is no desktop surface that drives the real agent.

## Vision

Launch one binary and it *is* Claude Desktop for SMARTAGENT: the sidebar shows your real pi sessions, the composer streams real tokens from the real model, tool cards light up live as `tasks`/`memory`/`browser` actually run, and the Chat / Cowork / Code tabs map onto real workflows — conversational chat, autonomous task work with live progress and artifacts, and per-project coding sessions rooted in `workspaces/`. Nothing in it is fake.

## Out of Scope

Reimplementing any agent capability in the GUI (pi + extensions stay the spine); a light theme; multi-window; image attachments and drag-drop uploads (v2); markdown rich rendering beyond simple text (headings/code fences styled minimally, v2 for full md); session tree/fork visualization (linear main-path view only); editing files in the Code tab (viewer + git status only, the agent does the editing); touching anything outside `desktop-agent/`.

## Principles

- Real over mock: every pixel of state originates from the pi process, session files, or repo binaries — never hardcoded demo data.
- The agent is the single source of capability; the GUI is a renderer + input device.
- Responsiveness: the UI thread never blocks on the child process or disk.
- Lean: smallest correct implementation; split files before 1000 lines.

## Constraints

- Changes confined to `desktop-agent/` (its `src/`, `Cargo.toml`, `ISA.md`). Nothing outside is modified.
- GUI toolkit stays `eframe`/`egui` 0.31 (already vendored in Cargo.lock); the only other dependency allowed is a path-dep on in-repo crates (e.g. `httpc` for JSON) — zero new crates.io deps.
- The agent child is spawned via the repo's `./pi` launcher with `--mode rpc` so extensions, AGENTS.md/AGENT_TOOLS.md injection, secrets token, and session dir all match the TUI exactly (parity by construction).
- LF-framed JSONL protocol per pi 0.80.3 `docs/rpc.md`; never split lines on U+2028/U+2029.
- No file over 1000 lines; no logic duplicated from crates (shell out to `target/release/*` binaries where repo data is needed).

## Goal

`cargo build --release -p desktop-agent` produces a windowed app whose three tabs (Chat, Cowork, Code) drive the real `./pi --mode rpc` subprocess with streaming text/thinking/tool events, whose sidebar lists and resumes real `.pi/sessions/*.jsonl`, and whose inspector shows real session stats, tool activity, and working files — verified by a scripted RPC round-trip and a live GUI run.

## Criteria

Foundation:
- [ ] ISC-1: `cargo build --release -p desktop-agent` exits 0
- [ ] ISC-2: No `.rs` file in `desktop-agent/src` exceeds 1000 lines
- [ ] ISC-3: `desktop-agent/Cargo.toml` adds no crates.io dep beyond existing `eframe` (path-deps on in-repo crates allowed)
- [ ] ISC-4: `git diff --stat` shows changes only under `desktop-agent/`
- [ ] ISC-5: All hardcoded demo data (fake sessions, canned messages, fake progress/artifacts/files) removed from src
- [ ] ISC-6: `cargo clippy -p desktop-agent --release` reports no errors (warnings tolerated)
- [ ] ISC-7: App still launches with usable UI when `./pi` is missing/unspawnable (error banner, not panic)
- [ ] ISC-8: Repo-root autodetect: binary locates repo root (walk up from exe/cwd until `./pi` + `.pi/` found), no hardcoded absolute path

RPC client (rpc.rs):
- [ ] ISC-9: Spawns `<root>/pi --mode rpc` with piped stdin/stdout/stderr and configurable cwd
- [ ] ISC-10: Writer serializes commands as single-line JSON + `\n` to child stdin
- [ ] ISC-11: Reader thread parses each stdout line as JSON and forwards on an mpsc channel
- [ ] ISC-12: Malformed stdout line is skipped with a logged warning, not a panic
- [ ] ISC-13: Records classified into response / event / extension_ui_request by `type` field
- [ ] ISC-14: Command `id` correlation: monotonically increasing ids; responses matched to pending commands
- [ ] ISC-15: `prompt` command sent with `streamingBehavior:"steer"` when a stream is active
- [ ] ISC-16: `abort` command wired and cancels a running turn (event stream shows termination)
- [ ] ISC-17: `new_session` command wired; state resets and sidebar refreshes
- [ ] ISC-18: `switch_session {sessionPath}` wired from sidebar click; transcript reloads from get_messages/entries
- [ ] ISC-19: `get_state` on connect populates model name, session file, thinking level
- [ ] ISC-20: `get_session_stats` after each `agent_end` populates tokens/cost/context gauge
- [ ] ISC-21: Child stderr drained on a thread (prevents pipe-full deadlock) and kept for error display
- [ ] ISC-22: Child exit detected; UI shows disconnected state and offers restart
- [ ] ISC-23: On app close, child stdin is dropped/closed so pi shuts down gracefully (no orphan process after quit)
- [ ] ISC-24: Reader thread triggers `egui::Context::request_repaint` on every record so streams render without mouse wiggle

Event → state reduction (agent.rs):
- [ ] ISC-25: `message_start` (user) appends user chat item with text content
- [ ] ISC-26: `message_update` with `text_delta` appends to the live assistant item's text
- [ ] ISC-27: `thinking_delta` accumulates into a collapsible thinking block on the live item
- [ ] ISC-28: `tool_execution_start` creates a tool card (name + args summary) keyed by toolCallId
- [ ] ISC-29: `tool_execution_update` replaces the card's partial output (accumulated semantics honored)
- [ ] ISC-30: `tool_execution_end` finalizes the card with ✓/✗, duration, and result preview
- [ ] ISC-31: `agent_end` clears streaming flag, re-enables composer, requests stats
- [ ] ISC-32: `message_end` for assistant stores final text (delta drift corrected from final message)
- [ ] ISC-33: Tool card args summary extracts the most informative field (command/path/query/action) not raw JSON dump
- [ ] ISC-34: `error` assistantMessageEvent surfaces as a visible error item, not silence
- [ ] ISC-35: `auto_retry_start` shows a retrying notice with attempt count
- [ ] ISC-36: `compaction_start`/`compaction_end` shown as a system notice in transcript
- [ ] ISC-37: `queue_update` renders queued steering/follow-up messages under the composer
- [ ] ISC-38: Streaming flag gates composer: send becomes steer while `isStreaming`
- [ ] ISC-39: Multiple sequential turns in one session render as separate transcript items (no bleed between turns)
- [ ] ISC-40: Tool result content >4KB truncated in card with byte count note (no unbounded UI growth)

Session browser (sessions.rs):
- [ ] ISC-41: Lists `.pi/sessions/*.jsonl`, newest-first by mtime
- [ ] ISC-42: Session title = latest `session_info.name`, else first user message text, else file stem
- [ ] ISC-43: Header line parsed for `cwd` (used to tag Code-tab sessions) and id
- [ ] ISC-44: Relative time label rendered (today/yesterday/N days) from mtime
- [ ] ISC-45: Transcript loader replays message entries (user/assistant/toolResult) into chat items for display
- [ ] ISC-46: toolResult entries pair with their assistant toolCall blocks by toolCallId when replaying
- [ ] ISC-47: Corrupt/empty session file skipped without panic
- [ ] ISC-48: Session list refreshes after new_session/switch_session and on a manual refresh control
- [ ] ISC-49: Loading a large session (>1MB) does not freeze the UI (loaded on spawn thread or capped)
- [ ] ISC-50: Sessions with cwd under `workspaces/` are badged with their project name

Sidebar (sidebar.rs):
- [ ] ISC-51: Tab pills Chat / Cowork / Code switch the central view
- [ ] ISC-52: "New session" starts a fresh RPC session for the active tab
- [ ] ISC-53: Real session list rendered (no fake Pinned/Scheduled demo rows)
- [ ] ISC-54: Clicking a session switches to it (switch_session + transcript reload)
- [ ] ISC-55: Active session highlighted
- [ ] ISC-56: Footer shows model name from get_state (not hardcoded "Sonnet 4"/"Max")
- [ ] ISC-57: Session list scrolls independently; sidebar layout stable at min window size
- [ ] ISC-58: Connection status dot (green connected / red dead child) visible in sidebar

Chat tab (chat.rs):
- [ ] ISC-59: Empty state greets with real username from $USER and prompts input
- [ ] ISC-60: User messages render as right-aligned bubbles, assistant as flowing text (Claude Desktop layout)
- [ ] ISC-61: Composer sends on Enter and on ↑ button; Shift+Enter inserts newline
- [ ] ISC-62: Live token streaming visible: text grows during the turn
- [ ] ISC-63: Thinking block renders collapsed by default, expandable
- [ ] ISC-64: Tool cards show live spinner state while running, ✓/✗ + ms when done
- [ ] ISC-65: Tool card click expands full args + result output (scrollable, monospace)
- [ ] ISC-66: Stop button visible during streaming, sends abort
- [ ] ISC-67: Composer disabled-state honesty: while disconnected, input explains why
- [ ] ISC-68: Transcript auto-scrolls to bottom on new content unless user scrolled up
- [ ] ISC-69: Model + thinking level shown at composer; reflects get_state
- [ ] ISC-70: Code fences in assistant text render in monospace with distinct background
- [ ] ISC-71: Transcript width capped (~780px) and centered like Claude Desktop
- [ ] ISC-72: Error events render as red-tinted system cards
- [ ] ISC-73: Resumed session renders its full replayed transcript before new input
- [ ] ISC-74: Send clears input and echoes the user message immediately (before first event arrives)

Cowork tab (cowork.rs):
- [ ] ISC-75: Task composer: describe work, sent as a prompt to the cowork session
- [ ] ISC-76: Progress pane derives live steps from this session's tool activity feed (chronological, running/done state)
- [ ] ISC-77: Kanban strip shows real `target/release/tasks board` output (parsed columns/counts), refreshed after turns
- [ ] ISC-78: Artifacts list = files created/edited this session (from write/edit tool args), deduped
- [ ] ISC-79: Clicking an artifact reveals its path (copy to clipboard)
- [ ] ISC-80: Cowork transcript view shares the chat renderer (same streaming fidelity)
- [ ] ISC-81: Working state banner: streaming turn shows elapsed time + current tool
- [ ] ISC-82: `tasks` binary missing → pane shows install hint, no panic

Code tab (code.rs):
- [ ] ISC-83: Project picker lists repo root + direct subdirs of `workspaces/` (dirs only)
- [ ] ISC-84: Selecting a project spawns/rebinds the Code RPC session with cwd = project path
- [ ] ISC-85: File tree of the selected project rendered (gitignore-light: skips .git, target, node_modules, .smartagent)
- [ ] ISC-86: Clicking a file shows read-only contents (monospace, capped at 200KB)
- [ ] ISC-87: Changed-files pane from `git status --porcelain` in project cwd, refreshed after each turn
- [ ] ISC-88: Chat pane bound to the project session (prompts run with the project as cwd)
- [ ] ISC-89: Active project name visible in header; sessions started here tagged with it (ISC-50 pairing)
- [ ] ISC-90: File tree lazy-expands directories (no full-repo walk on select)
- [ ] ISC-91: Non-UTF8/binary file selected → "binary file" notice, no panic
- [ ] ISC-92: Project with no git → changed-files pane says "not a git repo", rest works

Inspector (inspector.rs):
- [ ] ISC-93: Session stats: input/output tokens, cost, context % from get_session_stats
- [ ] ISC-94: Context gauge bar reflects contextUsage.percent
- [ ] ISC-95: Tool activity feed: last N tool executions with name, duration, ✓/✗
- [ ] ISC-96: Working files section = real files touched this session (from tool args)
- [ ] ISC-97: Statusline widget lines from extension_ui_request setWidget rendered verbatim (infra ⛭ / data ▦ rows)
- [ ] ISC-98: setStatus per-tool ephemeral statuses shown in activity area
- [ ] ISC-99: Session metadata: session file name, id, cwd shown
- [ ] ISC-100: Toggle button reopens inspector after close
- [ ] ISC-101: Inspector renders sanely when no session started yet (placeholders, not stale demo data)
- [ ] ISC-102: Queued messages (steer/follow-up) count visible while streaming

Extension UI dialogs:
- [ ] ISC-103: `extension_ui_request` select renders a modal with options; reply `{value}` on choice
- [ ] ISC-104: confirm renders modal with confirm/cancel; reply `{confirmed}`
- [ ] ISC-105: input/editor renders text modal; reply `{value}`
- [ ] ISC-106: Cancel/escape replies `{cancelled:true}` (agent never hangs)
- [ ] ISC-107: notify method renders a transient toast
- [ ] ISC-108: setTitle updates window title
- [ ] ISC-109: Dialog with `timeout` auto-dismisses locally when agent auto-resolves
- [ ] ISC-110: Multiple queued dialogs handled sequentially (no dropped ids)

Theme / UX polish:
- [ ] ISC-111: Claude Desktop dark palette retained (existing theme.rs constants) across all new views
- [ ] ISC-112: All three panes resize sanely at 900×600 minimum window
- [ ] ISC-113: Unicode-safe truncation everywhere (no byte-slice panics on Icelandic text)
- [ ] ISC-114: Idle app repaints on demand only (no 100% CPU spin; periodic repaint ≤2Hz when streaming off)
- [ ] ISC-115: Window title shows active session name
- [ ] ISC-116: `agent.desktop` launcher file updated to match binary name/path if needed

Anti-criteria:
- [ ] ISC-117: Anti: no hardcoded fake session/message/progress/artifact strings remain (`rg "double-charge|dark mode toggle|Weekly dependency"` in src → 0 hits)
- [ ] ISC-118: Anti: no file outside `desktop-agent/` modified (git status clean elsewhere)
- [ ] ISC-119: Anti: GUI never re-implements a tool (no HTTP calls, no semdb reads in GUI code; only pi RPC + `target/release/*` invocations + session-file reads)
- [ ] ISC-120: Anti: UI thread performs no blocking child I/O (all reads on threads; `wait()` never on UI thread)
- [ ] ISC-121: Anti: secrets never rendered (no SMARTAGENT_CALLER_TOKEN value in UI or logs)
- [ ] ISC-122: Anti: closing the app leaves no orphan pi process (pgrep after quit → none from this app)
- [ ] ISC-123: Anti: no /tmp usage; scratch only under repo `.scratch/` if needed
- [ ] ISC-124: Anti: no new crates.io dependency appears in Cargo.lock diff

End-to-end verification:
- [ ] ISC-125: Scripted RPC probe: spawn `./pi --mode rpc`, send prompt, receive message_update deltas + agent_end (protocol proven outside GUI)
- [ ] ISC-126: Live GUI run: send a chat prompt, receive streamed reply (screenshot/log evidence)
- [ ] ISC-127: Live GUI run: a prompt that triggers a tool (e.g. tasks board) shows a live tool card
- [ ] ISC-128: Resume flow: previous session opened from sidebar shows historical transcript
- [ ] ISC-129: Code tab: select a workspaces project, file tree + git status render
- [ ] ISC-130: codex fusion tester reports PASS on build + protocol + UI-state checks, spot-verified
- [ ] ISC-131: Prompt submitted while the child is still booting is queued and auto-sent on connect, with a visible "starting agent…" state (window appears immediately, never a dead cold start)
- [ ] ISC-132: Child death surfaces captured stderr excerpt in the error banner (diagnostic, not just "disconnected")

## Test Strategy

| isc | type | check | threshold | tool |
|---|---|---|---|---|
| 1-6 | build/audit | cargo build/clippy; wc -l; Cargo.toml grep; git diff --stat | exit 0 / limits | Bash |
| 7-8,22-23 | resilience | rename pi / kill child / quit app; pgrep | graceful, no orphans | Bash |
| 9-24 | protocol | scripted JSONL round-trip against live child | expected records | Bash probe |
| 25-40 | functional | code inspection + live GUI stream run | state matches events | Read + GUI run |
| 41-50 | functional | run session lister against real .pi/sessions | titles/order correct | Bash + GUI |
| 51-116 | UI | live GUI run + screenshots per tab | visible behavior | GUI run |
| 117-124 | anti | rg audits; git status; pgrep; Cargo.lock diff | zero hits | Bash |
| 125-130 | e2e | scripted probe + live runs + codex fusion tester | PASS verdicts | Bash/codex |

## Features

| name | satisfies | depends_on | parallelizable |
|---|---|---|---|
| json-reuse (httpc path dep) | 3,124 | — | no |
| rpc-client | 9-24,125 | json-reuse | no (core) |
| event-reducer | 25-40 | rpc-client | no |
| session-browser | 41-50 | json-reuse | yes |
| sidebar-real | 51-58 | session-browser, rpc-client | after core |
| chat-tab | 59-74 | event-reducer | after core |
| ext-ui-dialogs | 103-110 | rpc-client | after core |
| cowork-tab | 75-82 | event-reducer | after chat |
| code-tab | 83-92 | event-reducer, session-browser | after chat |
| inspector-real | 93-102 | event-reducer | after chat |
| polish+desktop-file | 111-116 | all | last |
| verification | 117-130 | all | last |

## Decisions

- 2026-07-02: Backend = pi RPC mode (`./pi --mode rpc`), not per-turn `-p` spawning and not reimplementation. Survey of vendored 0.80.3 + .refrepos/pi confirmed RPC is the documented embedding protocol with streaming events identical to the TUI's. Parity by construction: same launcher, same extensions, same session store.
- 2026-07-02: JSON via path-dep on in-repo `httpc` crate (`httpc::json`) — borrow, don't invent; zero new crates.io deps.
- 2026-07-02: One RPC child per tab-context (Chat @ repo root, Cowork @ repo root, Code @ selected project cwd), lazy-spawned — cwd is fixed at spawn, so per-context children instead of one shared child.
- 2026-07-02: E4 ISC floor met (130). Delegation: Explore agent (protocol survey) + codex fusion tester (repo-mandated). Forge/Cato codex build-agents skipped per standing user rule recorded in root ISA (Claude-family builds, codex tests); the fusion tester provides the cross-vendor audit surface.
- 2026-07-02: Cowork = autonomous-work surface mapped to real repo primitives (tool-activity progress, tasks kanban strip, session artifacts) rather than a cosmetic clone of claude.ai Cowork.

## Changelog

- 2026-07-02 — conjectured: parity would require the GUI to reimplement or shell out to each of the 20 tools individually. refuted by: pi 0.80.3 survey — `--mode rpc` streams the full event set and runs the same extensions the TUI does. learned: the GUI is a pure renderer over the RPC event stream; driving `./pi` itself is the only parity-safe design. criterion now: ISC-9/ISC-119 (spawn real launcher; GUI never reimplements a tool).
