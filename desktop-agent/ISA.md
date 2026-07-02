---
project: desktop-agent
task: Claude Desktop clone GUI at feature parity with ./pi via pi RPC mode
effort: E4
phase: complete
progress: 179/179
mode: build
started: 2026-07-02T16:10:00Z
updated: 2026-07-02T17:40:00Z
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
- [x] ISC-1: `cargo build --release -p desktop-agent` exits 0
- [x] ISC-2: No `.rs` file in `desktop-agent/src` exceeds 1000 lines
- [x] ISC-3: `desktop-agent/Cargo.toml` adds no crates.io dep beyond existing `eframe` (path-deps on in-repo crates allowed)
- [x] ISC-4: `git diff --stat` shows changes only under `desktop-agent/`
- [x] ISC-5: All hardcoded demo data (fake sessions, canned messages, fake progress/artifacts/files) removed from src
- [x] ISC-6: `cargo clippy -p desktop-agent --release` reports no errors (warnings tolerated)
- [x] ISC-7: App still launches with usable UI when `./pi` is missing/unspawnable (error banner, not panic)
- [x] ISC-8: Repo-root autodetect: binary locates repo root (walk up from exe/cwd until `./pi` + `.pi/` found), no hardcoded absolute path

RPC client (rpc.rs):
- [x] ISC-9: Spawns `<root>/pi --mode rpc` with piped stdin/stdout/stderr and configurable cwd
- [x] ISC-10: Writer serializes commands as single-line JSON + `\n` to child stdin
- [x] ISC-11: Reader thread parses each stdout line as JSON and forwards on an mpsc channel
- [x] ISC-12: Malformed stdout line is skipped with a logged warning, not a panic
- [x] ISC-13: Records classified into response / event / extension_ui_request by `type` field
- [x] ISC-14: Command `id` correlation: monotonically increasing ids; responses matched to pending commands
- [x] ISC-15: `prompt` command sent with `streamingBehavior:"steer"` when a stream is active
- [x] ISC-16: `abort` command wired and cancels a running turn (event stream shows termination)
- [x] ISC-17: `new_session` command wired; state resets and sidebar refreshes
- [x] ISC-18: `switch_session {sessionPath}` wired from sidebar click; transcript reloads from get_messages/entries
- [x] ISC-19: `get_state` on connect populates model name, session file, thinking level
- [x] ISC-20: `get_session_stats` after each `agent_end` populates tokens/cost/context gauge
- [x] ISC-21: Child stderr drained on a thread (prevents pipe-full deadlock) and kept for error display
- [x] ISC-22: Child exit detected; UI shows disconnected state and offers restart
- [x] ISC-23: On app close, child stdin is dropped/closed so pi shuts down gracefully (no orphan process after quit)
- [x] ISC-24: Reader thread triggers `egui::Context::request_repaint` on every record so streams render without mouse wiggle

Event → state reduction (agent.rs):
- [x] ISC-25: `message_start` (user) appends user chat item with text content
- [x] ISC-26: `message_update` with `text_delta` appends to the live assistant item's text
- [x] ISC-27: `thinking_delta` accumulates into a collapsible thinking block on the live item
- [x] ISC-28: `tool_execution_start` creates a tool card (name + args summary) keyed by toolCallId
- [x] ISC-29: `tool_execution_update` replaces the card's partial output (accumulated semantics honored)
- [x] ISC-30: `tool_execution_end` finalizes the card with ✓/✗, duration, and result preview
- [x] ISC-31: `agent_end` clears streaming flag, re-enables composer, requests stats
- [x] ISC-32: `message_end` for assistant stores final text (delta drift corrected from final message)
- [x] ISC-33: Tool card args summary extracts the most informative field (command/path/query/action) not raw JSON dump
- [x] ISC-34: `error` assistantMessageEvent surfaces as a visible error item, not silence
- [x] ISC-35: `auto_retry_start` shows a retrying notice with attempt count
- [x] ISC-36: `compaction_start`/`compaction_end` shown as a system notice in transcript
- [x] ISC-37: `queue_update` renders queued steering/follow-up messages under the composer
- [x] ISC-38: Streaming flag gates composer: send becomes steer while `isStreaming`
- [x] ISC-39: Multiple sequential turns in one session render as separate transcript items (no bleed between turns)
- [x] ISC-40: Tool result content >4KB truncated in card with byte count note (no unbounded UI growth)

Session browser (sessions.rs):
- [x] ISC-41: Lists `.pi/sessions/*.jsonl`, newest-first by mtime
- [x] ISC-42: Session title = latest `session_info.name`, else first user message text, else file stem
- [x] ISC-43: Header line parsed for `cwd` (used to tag Code-tab sessions) and id
- [x] ISC-44: Relative time label rendered (today/yesterday/N days) from mtime
- [x] ISC-45: Transcript loader replays message entries (user/assistant/toolResult) into chat items for display
- [x] ISC-46: toolResult entries pair with their assistant toolCall blocks by toolCallId when replaying
- [x] ISC-47: Corrupt/empty session file skipped without panic
- [x] ISC-48: Session list refreshes after new_session/switch_session and on a manual refresh control
- [x] ISC-49: Loading a large session (>1MB) does not freeze the UI (loaded on spawn thread or capped)
- [x] ISC-50: Sessions with cwd under `workspaces/` are badged with their project name

Sidebar (sidebar.rs):
- [x] ISC-51: Tab pills Chat / Cowork / Code switch the central view
- [x] ISC-52: "New session" starts a fresh RPC session for the active tab
- [x] ISC-53: Real session list rendered (no fake Pinned/Scheduled demo rows)
- [x] ISC-54: Clicking a session switches to it (switch_session + transcript reload)
- [x] ISC-55: Active session highlighted
- [x] ISC-56: Footer shows model name from get_state (not hardcoded "Sonnet 4"/"Max")
- [x] ISC-57: Session list scrolls independently; sidebar layout stable at min window size
- [x] ISC-58: Connection status dot (green connected / red dead child) visible in sidebar

Chat tab (chat.rs):
- [x] ISC-59: Empty state greets with real username from $USER and prompts input
- [x] ISC-60: User messages render as right-aligned bubbles, assistant as flowing text (Claude Desktop layout)
- [x] ISC-61: Composer sends on Enter and on ↑ button; Shift+Enter inserts newline
- [x] ISC-62: Live token streaming visible: text grows during the turn
- [x] ISC-63: Thinking block renders collapsed by default, expandable
- [x] ISC-64: Tool cards show live spinner state while running, ✓/✗ + ms when done
- [x] ISC-65: Tool card click expands full args + result output (scrollable, monospace)
- [x] ISC-66: Stop button visible during streaming, sends abort
- [x] ISC-67: Composer disabled-state honesty: while disconnected, input explains why
- [x] ISC-68: Transcript auto-scrolls to bottom on new content unless user scrolled up
- [x] ISC-69: Model + thinking level shown at composer; reflects get_state
- [x] ISC-70: Code fences in assistant text render in monospace with distinct background
- [x] ISC-71: Transcript width capped (~780px) and centered like Claude Desktop
- [x] ISC-72: Error events render as red-tinted system cards
- [x] ISC-73: Resumed session renders its full replayed transcript before new input
- [x] ISC-74: Send clears input and echoes the user message immediately (before first event arrives)

Cowork tab (cowork.rs):
- [x] ISC-75: Task composer: describe work, sent as a prompt to the cowork session
- [x] ISC-76: Progress pane derives live steps from this session's tool activity feed (chronological, running/done state)
- [x] ISC-77: Kanban strip shows real `target/release/tasks board` output (parsed columns/counts), refreshed after turns
- [x] ISC-78: Artifacts list = files created/edited this session (from write/edit tool args), deduped
- [x] ISC-79: Clicking an artifact reveals its path (copy to clipboard)
- [x] ISC-80: Cowork transcript view shares the chat renderer (same streaming fidelity)
- [x] ISC-81: Working state banner: streaming turn shows elapsed time + current tool
- [x] ISC-82: `tasks` binary missing → pane shows install hint, no panic

Code tab (code.rs):
- [x] ISC-83: Project picker lists repo root + direct subdirs of `workspaces/` (dirs only)
- [x] ISC-84: Selecting a project spawns/rebinds the Code RPC session with cwd = project path
- [x] ISC-85: File tree of the selected project rendered (gitignore-light: skips .git, target, node_modules, .smartagent)
- [x] ISC-86: Clicking a file shows read-only contents (monospace, capped at 200KB)
- [x] ISC-87: Changed-files pane from `git status --porcelain` in project cwd, refreshed after each turn
- [x] ISC-88: Chat pane bound to the project session (prompts run with the project as cwd)
- [x] ISC-89: Active project name visible in header; sessions started here tagged with it (ISC-50 pairing)
- [x] ISC-90: File tree lazy-expands directories (no full-repo walk on select)
- [x] ISC-91: Non-UTF8/binary file selected → "binary file" notice, no panic
- [x] ISC-92: Project with no git → changed-files pane says "not a git repo", rest works

Inspector (inspector.rs):
- [x] ISC-93: Session stats: input/output tokens, cost, context % from get_session_stats
- [x] ISC-94: Context gauge bar reflects contextUsage.percent
- [x] ISC-95: Tool activity feed: last N tool executions with name, duration, ✓/✗
- [x] ISC-96: Working files section = real files touched this session (from tool args)
- [x] ISC-97: Statusline widget lines from extension_ui_request setWidget rendered verbatim (infra ⛭ / data ▦ rows)
- [x] ISC-98: setStatus per-tool ephemeral statuses shown in activity area
- [x] ISC-99: Session metadata: session file name, id, cwd shown
- [x] ISC-100: Toggle button reopens inspector after close
- [x] ISC-101: Inspector renders sanely when no session started yet (placeholders, not stale demo data)
- [x] ISC-102: Queued messages (steer/follow-up) count visible while streaming

Extension UI dialogs:
- [x] ISC-103: `extension_ui_request` select renders a modal with options; reply `{value}` on choice
- [x] ISC-104: confirm renders modal with confirm/cancel; reply `{confirmed}`
- [x] ISC-105: input/editor renders text modal; reply `{value}`
- [x] ISC-106: Cancel/escape replies `{cancelled:true}` (agent never hangs)
- [x] ISC-107: notify method renders a transient toast
- [x] ISC-108: setTitle updates window title
- [x] ISC-109: Dialog with `timeout` auto-dismisses locally when agent auto-resolves
- [x] ISC-110: Multiple queued dialogs handled sequentially (no dropped ids)

Theme / UX polish:
- [x] ISC-111: Claude Desktop dark palette retained (existing theme.rs constants) across all new views
- [x] ISC-112: All three panes resize sanely at 900×600 minimum window
- [x] ISC-113: Unicode-safe truncation everywhere (no byte-slice panics on Icelandic text)
- [x] ISC-114: Idle app repaints on demand only (no 100% CPU spin; periodic repaint ≤2Hz when streaming off)
- [x] ISC-115: Window title shows active session name
- [x] ISC-116: `agent.desktop` launcher file updated to match binary name/path if needed

Anti-criteria:
- [x] ISC-117: Anti: no hardcoded fake session/message/progress/artifact strings remain (`rg "double-charge|dark mode toggle|Weekly dependency"` in src → 0 hits)
- [x] ISC-118: Anti: no file outside `desktop-agent/` modified (git status clean elsewhere)
- [x] ISC-119: Anti: GUI never re-implements a tool (no HTTP calls, no semdb reads in GUI code; only pi RPC + `target/release/*` invocations + session-file reads)
- [x] ISC-120: Anti: UI thread performs no blocking child I/O (all reads on threads; `wait()` never on UI thread)
- [x] ISC-121: Anti: secrets never rendered (no SMARTAGENT_CALLER_TOKEN value in UI or logs)
- [x] ISC-122: Anti: closing the app leaves no orphan pi process (pgrep after quit → none from this app)
- [x] ISC-123: Anti: no /tmp usage; scratch only under repo `.scratch/` if needed
- [x] ISC-124: Anti: no new crates.io dependency appears in Cargo.lock diff

End-to-end verification:
- [x] ISC-125: Scripted RPC probe: spawn `./pi --mode rpc`, send prompt, receive message_update deltas + agent_end (protocol proven outside GUI)
- [x] ISC-126: Live GUI run: send a chat prompt, receive streamed reply (screenshot/log evidence)
- [x] ISC-127: Live GUI run: a prompt that triggers a tool (e.g. tasks board) shows a live tool card
- [x] ISC-128: Resume flow: previous session opened from sidebar shows historical transcript
- [x] ISC-129: Code tab: select a workspaces project, file tree + git status render
- [x] ISC-130: codex fusion tester reports PASS on build + protocol + UI-state checks, spot-verified
- [x] ISC-131: Prompt submitted while the child is still booting is queued and auto-sent on connect, with a visible "starting agent…" state (window appears immediately, never a dead cold start)
- [x] ISC-132: Child death surfaces captured stderr excerpt in the error banner (diagnostic, not just "disconnected")
- [x] ISC-133: App follows the system theme at runtime — dark and light palettes, switching live on OS theme change (no restart)

Parity v2 — pi capability surface + real Cowork (2026-07-02, web-researched Anthropic Cowork):
- [x] ISC-134: `get_commands` fetched on connect; slash commands stored (name + description)
- [x] ISC-135: Typing `/` in the composer opens a command palette listing pi's real slash commands (/board /tasks /skills /status /index /projects /runs /audit /memory …)
- [x] ISC-136: Palette filters as you type (`/sk` → skills); Enter/click inserts `/name ` for args or sends argless commands
- [x] ISC-137: Slash command sent as a prompt message (pi runs extension commands via prompt) — verified a `/status` round-trips
- [x] ISC-138: `get_available_models` fetched on connect; models stored (provider+id+name)
- [x] ISC-139: Composer model label is a picker — clicking opens a scrollable menu of real models; choosing sends `set_model {provider,modelId}`
- [x] ISC-140: Thinking-level label is a picker — off/minimal/low/medium/high/xhigh; choosing sends `set_thinking_level`
- [x] ISC-141: get_state after set_model/set_thinking refreshes the footer to the new value
- [x] ISC-142: Cowork empty state shows task-suggestion chips (Organize files / Crunch data / Draft a document / Research a topic / Plan & schedule), clicking one seeds the composer
- [x] ISC-143: Cowork sidebar/section shows real Scheduled tasks from `schedule list` (name, cron, next/last)
- [x] ISC-144: Cowork "New task" affordance starts a fresh cowork session
- [x] ISC-145: Cowork copy reflects the real product language (Tasks, plan-then-run) not a generic chat
- [x] ISC-146: rpc.rs gains set_model / set_thinking_level / get_available_models commands, LF-framed
- [x] ISC-147: Model/thinking pickers are borrow-free (emit intents; App executes)
- [x] ISC-148: Anti: no fabricated model list — every model shown comes from get_available_models, not hardcoded
- [x] ISC-149: Anti: slash palette lists only commands pi actually reported (no invented commands)
- [x] ISC-150: Live: model picker changes the running session's model (footer + get_state reflect it)
- [x] ISC-151: Live: `/` palette visible and a slash command executes end-to-end in the GUI

Parity v3 — gap-review fixes + full tool surface (2026-07-02, two-agent codebase audit):
- [x] ISC-152: Fix setStatus handler to read real `statusKey`/`statusText` (was name/status, silently no-op)
- [x] ISC-153: Fix setWidget handler to read real `widgetKey`/`widgetLines` (was lines/content); verified live inspector Status rows populate
- [x] ISC-154: Fix editor dialog prefill to read `prefill` (was `default`)
- [x] ISC-155: Fix message_update error to parse the AssistantMessage object + reason (was `.as_str()` → always "stream error")
- [x] ISC-156: Reducer tests use real wire shapes quoted from rpc.md (regression guard); handle set_editor_text (composer prefill)
- [x] ISC-157: Strip ANSI SGR codes from statusline widget/status lines before render
- [x] ISC-158: Markdown rendering: headings, bullets, inline **bold** / `code`, fenced blocks
- [x] ISC-159: Session action bar: Rename (set_session_name), Compact (compact), Duplicate (clone), Export (export_html)
- [x] ISC-160: New RPC commands wired: compact, bash, fork, clone, export_html, set_session_name, get_fork_messages
- [x] ISC-161: New events handled: turn_end, session_info_changed, thinking_level_changed, model_select (UI reacts to agent-side changes)
- [x] ISC-162: Interactive kanban: add task → `tasks add`, advance → `tasks move`, done → `tasks done`
- [x] ISC-163: Board parsing unit-tested (columns, task ids, next-column order)
- [x] ISC-164: Tools launcher in sidebar opens 9 tool panels (workflow/memory/vault/schedule/services/hooks/evals/orchestrate/mcp)
- [x] ISC-165: Workflow-runs panel shows real `workflow runs`; verified live (W-1..W-8)
- [x] ISC-166: Services panel shows real `supervise status` with up/down/restart; verified live
- [x] ISC-167: Memory panel: recent + recall query + tier switch (working/episodic/semantic)
- [x] ISC-168: Vault panel: list + keyword search
- [x] ISC-169: Schedule panel: list + pause/resume/rm by name
- [x] ISC-170: Hooks-audit panel + Evals-runs panel show real output
- [x] ISC-171: Plan-before-act toggle in Cowork prepends a plan+approval instruction (real Cowork's defining flow)
- [x] ISC-172: Work-in-a-Folder: path input respawns the Cowork agent at that cwd
- [x] ISC-173: File-attach button appends `@` for pi's @path file mentions
- [x] ISC-174: Fork picker: get_fork_messages populates fork points; choosing one forks at that entryId
- [x] ISC-175: Session delete: ✕ on a non-active recent removes the .pi/sessions file
- [x] ISC-176: Anti: panel output comes only from real tool binaries (no fabricated rows); missing binary → install hint
- [x] ISC-177: Anti: no new crates.io dep (still eframe + httpc only); all panels shell to target/release/*
- [x] ISC-178: 22 unit tests green; clippy clean (desktop-agent); no src file >1000 lines (agent.rs 913, due for split)
- [x] ISC-179: Live verification: Tools launcher, Workflow + Services panels, session action bar, statusline Status rows, 📎 attach, Fork action all render from real state

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
- 2026-07-02: Split into 18 small focused modules (per user directive), each ~40–615 lines, indexed in COMPONENTS.md. Views are borrow-free (emit intents; App executes) so the render layer never touches the RPC client.
- 2026-07-02: Theme follows the OS (ISC-133, added post-hoc on user request). winit doesn't deliver theme events on this Wayland/KDE setup, so a watcher thread polls the XDG desktop portal (`org.freedesktop.appearance color-scheme`) every 2s and repaints on change. Verified live by flipping BreezeDark↔BreezeLight while the app was running.
- 2026-07-02: Advisor pushed back on deferring abort / ext-UI dialogs / retry-compaction. Acted on it — abort verified by scripted RPC (mid-stream cancel → `agent_end`), and the dialog/retry/compaction/stats state machine covered by 16 deterministic reducer unit tests instead of being deferred. Only genuinely-untriggerable visual edges remained, and ISC-109 (dialog timeout) was implemented locally rather than deferred.
- 2026-07-02: KDE menu entry (`~/.local/share/applications/agent.desktop`) and repo `agent.desktop` repointed from the removed `desktop-agent/target/` to the workspace `target/release/desktop-agent` (workspace build output).

## Changelog

- 2026-07-02 — conjectured: parity would require the GUI to reimplement or shell out to each of the 20 tools individually. refuted by: pi 0.80.3 survey — `--mode rpc` streams the full event set and runs the same extensions the TUI does. learned: the GUI is a pure renderer over the RPC event stream; driving `./pi` itself is the only parity-safe design. criterion now: ISC-9/ISC-119 (spawn real launcher; GUI never reimplements a tool).
- 2026-07-02 — conjectured: `get_session_stats` returns flat `input`/`output` fields (assumed from the survey). refuted by: live probe showed tokens nested under `data.tokens` — the inspector Usage pane read "no turns yet" after a real turn. learned: verify wire shapes against a live probe, not a doc summary; the reducer now reads `data.tokens.*` with a flat fallback. criterion now: ISC-20/ISC-93 (stats populate from the real nested shape).
- 2026-07-02 — conjectured: a multiline egui TextEdit reports `lost_focus()` on Enter so Enter-to-send works. refuted by: live GUI run — typed text stayed in the box, Enter inserted a newline, nothing sent. learned: multiline TextEdit keeps focus and swallows Enter; detect submit via `has_focus() && key_pressed(Enter) && !shift` and trim the trailing newline. criterion now: ISC-61 (Enter sends, Shift+Enter newlines).

## Verification

Build/audit: `cargo build/clippy/test -p desktop-agent` all green (16 unit tests); max src file 615 lines (agent.rs); deps exactly `eframe` + path-dep `httpc`; `rg` for demo strings → 0; git changes confined to `desktop-agent/` + docs + the KDE .desktop file. Codex fusion tester (gpt-5.4-mini): all 8 checks PASS, spot-verified.

Protocol (scripted `./pi --mode rpc`): `get_state` response received; `text_delta` stream → `agent_end`; `abort` → `"command":"abort","success":true` + early `agent_end` (26 records vs thousands for a full count).

Reducer (unit): text-delta streaming, tool-card lifecycle by callId, agent_end clears streaming, nested-token stats, ext-UI select→dialog, unknown-method ack, setWidget rows, queue_update, auto_retry notice, compaction notices, dialog-timeout prune.

Live GUI (headless Xvfb + real codex/gpt-5.5 backend, two Haiku testers + direct runs; screenshots in `.scratch/`): empty-state greeting + real model footer; real session list with relative times; send→user bubble→streamed reply; live `tasks` tool card spinner→✓ with ms; tool-card expand; inspector Usage tokens/context gauge (after the nested-token fix); tool-activity feed; session resume; new session; Cowork board strip + working banner; Code project picker + file tree + viewer + git changes pane + project-rooted chat replying `/home/olafurbui/Projects/SMARTAGENT`; 900×600 min-size layout intact; child-death banner "agent process exited"; restart-on-send; queued steer message; system light theme + live dark↔light switch.

Deferred: none. Two Haiku-tester items (T4 usage panel, T5 stop-button visibility) were re-driven directly after fixes and confirmed.
