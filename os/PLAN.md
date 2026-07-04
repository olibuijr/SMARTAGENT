# SMARTAGENT OS — feature map, plan, and parallel-build assignment

One Dioxus fullstack app (web · desktop · mobile), gateway/fleet as backend.

## Feature map (parity source: desktop-agent + pi TUI)

**desktop-agent (egui):**
- Chat: streaming chat with pi; bubbles, code fences, thinking, live tool cards, banners.
- Cowork ("Tasks"): kanban board (add/advance/done), suggestions, Scheduled, working banner, task composer.
- Code: project picker (`workspaces/`), lazy file tree, read-only viewer, git changes, project-rooted chat.
- Sidebar: session list + resume, new session, connection dot, model footer.
- Inspector: session actions (rename/compact/duplicate/export/fork), usage + context gauge, statusline rows, tool activity, files/artifacts.
- Tool panels: workflow/memory/vault/schedule/services/hooks/evals command-center.
- Dialogs: extension-UI modals (select/confirm/input/editor). System dark/light theme.

**pi TUI:** 3-row statusline (workspace/data/infra health), `/team` fleet panel (agent cards, context bars, task subcards, running workflows), `/board`, `/status`, `/runs`, `/skills`, `/memory`, `/sab` visual browser.

**Cowork target (Claude Cowork / Hermes):** connect mail, inbox triage, daily agenda, autonomous life/task management.

## Done
- Pipeline (web+phone, tap-reactive) · Chat streams jeeves over gateway TCP bridge · pixel brand.

## Parallel build — module contracts (each agent owns ONLY its files)

To keep merges clean: an agent touches **only** its module `.rs` + its `assets/<x>.css`,
exposes **one** `#[component] pub fn X() -> Element`, and does **not** edit `app.rs`,
`main.rs`, or other agents' files. The orchestrator wires components into `app.rs`
and links CSS at merge time. Shared read-only API: `net.rs` (gateway client).

| Agent | Files (owns) | Deliverable |
|-------|--------------|-------------|
| **A — Sessions** | `sessions.rs`, `assets/sessions.css` | Persistent chat-session store (create / auto-name from first msg / rename / delete), an in-app themed dropdown to switch/manage; `pub fn SessionBar()` + a `Sessions` store API other tabs read. Persist on device (dioxus storage / a JSON file under app data). |
| **B — Assistant-UI** | `blocks.rs`, `assets/blocks.css` | Rich transcript blocks: streaming text, collapsible **thinking**, **tool-call cards** (running→green ✓/red ✗ from gateway `🛠 <tool>` lines), **file-edit diff** (red/green). `pub fn Block(kind)`. Parser from the event stream. |
| **C — Cowork** | `cowork.rs`, `assets/cowork.css` | Cowork tab like Claude Cowork/Hermes: **mail** inbox list (via a gateway op to himalaya), daily agenda, tasks board (add/advance/done via gateway → `tasks` binary), scheduled items. `pub fn Cowork()`. |
| **D — Code** | `code.rs`, `assets/code.css` | Code tab: pick a repo under `workspaces/`, lazy file tree + read-only viewer + git status, project-rooted agent chat via gateway RPC. `pub fn Code()`. |
| **E — Theme/Shell** | `theme.rs`, `assets/theme.css`, Android native cfg | Fix webview **not following system dark/light** (native WebView `setAlgorithmicDarkeningAllowed` / DayNight manifest theme via dx Android config), real safe-area insets, light+dark palettes. `pub fn apply_theme()`. |

Orchestrator (me): owns `app.rs` (shell, Rail, tab routing, meta), `net.rs` (extend
for richer events as B needs), and any gateway-side additions (mail op for C, tool/
thinking event enrichment for B). Reviews each branch, wires components, merges,
builds the APK, verifies on the phone.

## Build/verify
`RUSTC_WRAPPER="" dx build --platform android --target aarch64-linux-android`;
install+launch+screenshot via `padb` (skills/PhoneControl). Gateway bridge:
`gateway_tcp_addr` token-gated over LAN/VPN.
