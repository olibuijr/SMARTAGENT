# desktop-agent — component index

Claude-Desktop-style GUI over the real `./pi` agent (RPC mode). Every module is
small and single-purpose; keep it that way — split before a file grows past
~350 lines, and add the new module to this index in the same commit.

## Backend (no UI)

| Module | Lines* | Job |
|--------|-------|-----|
| `src/root.rs` | ~70 | Repo-root autodetection (walk up until `./pi` + `.pi/`), derived paths, username, clock |
| `src/jsonw.rs` | ~130 | JSON serializer + value builders over `httpc::json`, unicode-safe truncation |
| `src/rpc.rs` | ~230 | Spawn `./pi --mode rpc`, LF-JSONL reader/stderr threads, typed send helpers, graceful shutdown |
| `src/agent.rs` | ~770 | Event → state reducer: transcript items, tool cards, stats, dialogs, statusline, files (incl. reducer unit tests) |
| `src/sessions.rs` | ~510 | `.pi/sessions/*.jsonl` browser: list metas (title/cwd/project/mtime) + transcript replay |
| `src/conn.rs` | ~150 | One agent connection (child + state + input) per tab context: ensure/pump/send/switch |
| `src/data.rs` | ~180 | App-side repo data: session refresh, workspaces projects, lazy file tree, git status, tasks board |

## UI

| Module | Lines* | Job |
|--------|-------|-----|
| `src/main.rs` | ~250 | App shell: module tree, tab state, per-frame pump, panel layout, emit execution |
| `src/emit.rs` | ~100 | `Emit` intent enum + executor — views stay borrow-free |
| `src/theme.rs` | ~130 | System-following palette (dark/light via XDG portal watcher) + egui style |
| `src/icons.rs` | ~60 | Embeds JetBrains Mono Nerd Font (`assets/NerdFont.ttf`) as an egui fallback family; named Font Awesome icon constants |
| `src/sidebar.rs` | ~180 | Logo, tab pills, new session, real session list, connection dot, model footer |
| `src/transcript.rs` | ~200 | Shared renderer: bubbles, assistant text with code fences, thinking, live tool cards, banners |
| `src/composer.rs` | ~100 | Shared input bar: Enter-send / Shift+Enter newline, Stop while streaming, model footer |
| `src/chat.rs` | ~60 | Chat tab: greeting empty state + transcript + composer |
| `src/cowork.rs` | ~180 | Cowork "Tasks" tab: suggestions, Scheduled section, working banner, interactive board, transcript, task composer |
| `src/board.rs` | ~180 | Interactive kanban: parse `tasks board`, add / advance / done via the real `tasks` binary |
| `src/code.rs` | ~230 | Code tab: project picker, lazy file tree, read-only viewer, git changes, project-rooted chat |
| `src/inspector.rs` | ~260 | Right panel: session action bar (rename/compact/duplicate/export/fork), info, usage + context gauge, statusline rows, tool activity, files/artifacts |
| `src/panels.rs` | ~180 | Tools command-center: launcher + panels for workflow/memory/vault/schedule/services/hooks/evals, each shelling to its real binary |
| `src/dialogs.rs` | ~130 | Extension-UI modals (select/confirm/input/editor) + escape-to-cancel |

*approximate, at time of writing — regenerate with `wc -l src/*.rs`.

## Data flow

```
./pi --mode rpc (child per tab-context: Chat/Cowork @ repo root, Code @ project)
   │  stdout JSONL              stdin JSONL
   ▼                              ▲
rpc.rs reader thread ──mpsc──► conn.pump() ──► agent.rs reducer ──► AgentState
                                                                      │
sidebar/chat/cowork/code/inspector render(AgentState) ──► Vec<Emit> ──┘
                                          (main.rs executes emits per frame)
```

Rules: views never touch `RpcClient` directly (emit intents instead); all agent
capability lives in pi + extensions — the GUI only renders and invokes repo
binaries (`tasks board`, `git status`) for panel data. System of record:
`ISA.md` in this directory.
