# SMARTAGENT OS — app roadmap

One Dioxus fullstack app (web · desktop · mobile), gateway/fleet as backend.
Parity target: desktop-agent + what Claude Desktop / Hermes do.

## Done
- (a) Pipeline proof: shell on web + phone, tap-reactive. ✓
- (b) Chat streams `jeeves` over the gateway TCP bridge. ✓
- Pixel brand: logo header, jeeves avatar, pixel tab icons, Press Start font. ✓

## Now (in order)

1. **Safe area + system theme** — respect status-bar/nav insets
   (`viewport-fit=cover` + `env(safe-area-inset-*)`); follow the OS light/dark
   theme (`prefers-color-scheme`, `color-scheme` meta); safe-area regions match
   the active theme.

2. **Persistent chat sessions** — Chat is currently one ephemeral jeeves
   session. Add: a session list, **create new**, **dynamic auto-naming** (from
   first message / summary), **rename**, **delete**, all via an in-app dropdown
   styled to the theme. Persist per-device (local store now; gateway/semdb-backed
   later so sessions sync across surfaces).

3. **Assistant-UI components** — rich chat rendering: streaming text, collapsible
   **thinking** blocks, **tool-call cards** (running → green ✓ / red ✗; parse the
   gateway's `🛠 <tool>` info lines), and **file-edit diffs** (red/green lines;
   needs the gateway to forward tool I/O — later sub-step).

4. **Cowork** — like Claude Cowork / Hermes: connect **mail** (himalaya via the
   fleet), manage day-to-day life — inbox triage, tasks board, scheduled items,
   daily agenda — driven by the fleet's real capabilities.

5. **Code** — connect to the SMARTAGENT RPC to work on repos under
   `workspaces/`: pick a project, project-rooted coding agent session, file
   tree + read-only viewer + git status (agent does the editing), per
   desktop-agent's Code tab.

## Notes
- Build: `RUSTC_WRAPPER="" dx build --platform android --target aarch64-linux-android`
  (see skills/PhoneControl). Verify every step on the real device.
- Gateway bridge: `gateway_tcp_addr` (LAN/VPN), token-gated.
