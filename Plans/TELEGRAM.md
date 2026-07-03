# Telegram channel — ported from Hermes (concept, not code)

> Goal: SMARTAGENT talks to Óli over Telegram — inbound messages reach the agent, agent replies go back. **Telegram ONLY** (no other chat platform). Borrow the concept from `~/Projects/hermes-agent/plugins/platforms/telegram/` (adapter.py 7473 lines — do NOT port it; it's Python/httpx + forum-topic/DM-anchor edge cases we don't need). Port the ~20 lines of real protocol.

## What Telegram Bot API actually is (the portable core)

Plain HTTPS to `https://api.telegram.org/bot<TOKEN>/<method>`. Our `httpc` already does HTTPS (openssl s_client). Two methods carry everything:

- **`getUpdates?offset=<N>&timeout=25`** — long-poll inbound. Returns `result: [{update_id, message:{chat:{id}, text, from:{id,username}}}]`. Ack by passing `offset = max(update_id)+1` next call. 25s server-side long-poll (well under httpc's timeout).
- **`sendMessage`** (POST JSON `{chat_id, text, parse_mode?, message_thread_id?}`) — outbound.

From Hermes, the two rules worth keeping:
- **MAX_MESSAGE_LENGTH = 4096** — split longer replies into ≤4096-char chunks (prefer newline boundaries), send sequentially.
- **MarkdownV2 escaping** — escape `_*[]()~\`>#+-=|{}.!` when using `parse_mode=MarkdownV2`; or just send plain text (no parse_mode) for v1 and skip escaping.

Drop everything else Hermes has (DoH fallback IPs, forum topics, DM-topic anchors, PTB pool resets, sticky-IP transport) — those solve problems we don't have.

## SMARTAGENT shape

- **`crates/telegram`** (pure Rust, std + `httpc` + `semdb` only):
  - `telegram poll` — one getUpdates cycle: read stored offset from a `telegram.semdb` table, long-poll, print/return new messages as JSON lines, persist new offset. (The gateway/agent loop calls this; no bot framework.)
  - `telegram send --chat <id> --text <msg>` — sendMessage with 4096 chunking.
  - `telegram listen` — repeat `poll` on an interval (for a supervised service), emitting each inbound message to stdout (the gateway can pipe these to `gateway send`).
  - Bot token via `secrets get` (policy-gated) — NEVER a flag or env literal in code/docs. Chat allow-list in `config/smartagent.conf` (`telegram_allowed_chats`) so only Óli's chat is honored (Hermes `_is_callback_user_authorized` concept).
- **`extensions/telegram.ts`** — thin glue, `telegram` tool (send/poll/listen).
- **Wiring:** a supervised `telegram listen` feeds inbound → `gateway send --agent main`, and the agent's reply routes back via `telegram send`. (v1 can be manual send/poll; the listen→gateway bridge is step 2.)
- **Table:** `data/telegram.semdb` — offset row + message log (no vector v1).

## Out of scope
Any non-Telegram platform (explicit: Telegram only). Forum topics, DM-topic anchors, inline keyboards/callbacks, media beyond text, DoH/sticky-IP transport, message editing/streaming continuations.

## Telegram streaming policy

- The gateway reads pi RPC stdout in `crates/gateway/src/child.rs`. The current event pump forwards only:
  - assistant `text_delta` as user-visible reply text,
  - `tool_execution_start` as a tool/progress event,
  - final `message_end` / `turn_end` usage as completion metadata.
- pi RPC mode (`dist/modes/rpc/rpc-mode.js`) forwards raw session events, but SMARTAGENT's gateway deliberately does **not** forward assistant `thinking` content to Telegram. The interactive TUI has a local "toggle thinking blocks" affordance; Telegram is a remote chat channel and should not expose hidden chain-of-thought.
- Provider support is inconsistent: AkurAI Router marks codex/claude models as reasoning-capable, opencode-go models as non-reasoning, and upstreams may expose only budgets/usage rather than streamable reasoning text. Treat reasoning tokens as private model/internal data unless a provider explicitly offers a safe summary field.
- Safe alternative for Telegram: stream visible assistant text, tool-call status (e.g. wrench/progress messages), and concise progress summaries such as "thinking…", "using <tool>…", and "still working…". Do not stream hidden reasoning tokens or chain-of-thought.
- Robust delivery strategy: keep live edits rate-limited to ~1 edit / 1.5s; retry transient Bot API failures/rate limits with short backoff; final replies are split into ordered ≤4000-character chunks by editing the placeholder with chunk 1 and sending follow-up chunks 2..N. This avoids Telegram's 4096-character limit, keeps long answers readable, and prevents duplicate first chunks.

## Telegram response formatting templates

Use these templates for user-visible Telegram replies so long agent output,
slash-command output, and hook reports stay scannable on mobile. Use plain text
by default (`parse_mode` off): Telegram Markdown is fragile with dynamic
agent/tool output, table pipes, backticks, underscores, and long IDs. Opt into
Markdown only after every dynamic field is escaped; never use MarkdownV2 unless
fields are escaped with Telegram's full MarkdownV2 escape set.

### Inventory of response types

| Type | Current source/examples | Template |
|---|---|---|
| Agent final reply | `stream_reply` final gateway output, normal chat prompts | `💬 Answer` heading, then 1-3 concise paragraphs. If follow-up work is needed, end with `Next: …`. |
| Streaming progress | `ProgressEvent`, tool-status frames, placeholder edits | One-line status with emoji (`🧭 Planning…`, `🔧 Using <tool>…`, `⏳ Waiting…`, `✅ Verifying…`) plus at most one short detail line or visible answer snapshot. |
| Slash-command output | `/board`, `/tasks`, `/status`, `/agents`, `/runs`, `/skills`, `/memory` via `slash_command` | `/<command>` heading, then clipped plain-text tool output. Preserve existing task/status line breaks; avoid code fences unless output is otherwise ambiguous. |
| Help/command catalog | `/help`, `/commands`, `/start`, `command_help` | `SMARTAGENT Telegram commands` heading followed by bullets: `• /cmd — description`. |
| Task creation / kanban planning | Agent replies that create, plan, or update board tasks | `📋 Task` heading; fields on separate lines: `ID: T-n`, `Priority: pX`, `State: ready/doing`, `Criteria: checked/total`, then criteria bullets when useful. |
| Board/status reports | `/board`, `/tasks`, `/status`, done hook, status-report workflow | `📊 Status` heading; grouped sections (`Ready`, `Doing`, `Review`, `Blocked`) using bullets. Include `Evidence:` and `Next:` sections for workflow/status reports. |
| Blockers / denials | `safe_denial`, blocked-task reports, unavailable commands | `⚠ Blocked` or `⛔ Not available` heading; one sentence reason; optional `Next: …`. Avoid exposing allow-list/admin internals. |
| Errors | command failures, gateway unavailable, Telegram API failures | `❌ Error` heading; human-readable problem line; optional retry guidance. Do not include raw secrets, tokens, or giant stderr dumps. |
| Memory recall | `/memory <query>` | `🧠 Memory` heading; bullets with the recalled fact first and source/id only when useful. If empty, say `No matching memory found.` |
| Memory write/reset | `/remember`, `/reset`, context injection summaries | `🧠 Memory updated` or `🧹 Context reset`; one sentence with scope (`this chat/thread`) and count when available. |
| Model selection | `/model`, callback menu | `🤖 Model` heading; current model line; numbered choices. Inline keyboard buttons mirror the same order. |
| Stop/cancel | `/stop` and active stream cancellation | `⏹ Stopped` heading; confirm only this chat/thread was affected. |

### Plain text vs Markdown tables

- Prefer plain text bullets for dynamic agent/tool output, command catalogs,
  errors, blockers, task updates, progress frames, long task titles, stack
  traces, and anything containing Markdown control characters.
- Use a Markdown-style table only when the data is small, rectangular, already
  normalized, and useful to compare across rows (for example: `ID | Priority |
  State | Title`). Keep tables to four columns or fewer with compact headers.
- If a table would wrap badly on mobile, fall back to bullets grouped by heading
  (`READY`, `DOING`, `REVIEW`). Current `/board` and `/tasks` raw CLI output may
  be relayed in plain form until a formatter converts it.
- In-progress streaming edits should stay plain text unless all dynamic content
  is escaped. Finished replies may use Markdown only when the formatter owns all
  markup and escapes dynamic fields.

## Response formatting

Structured response templates for command output, task updates, blockers, errors, memory recall, slash commands, and streaming progress live in [TELEGRAM_RESPONSE_TEMPLATES.md](./TELEGRAM_RESPONSE_TEMPLATES.md).

## Verify
`telegram send` delivers to Óli's chat (he sees it); `telegram poll` returns a message he sent; token only via secrets; non-allowlisted chat ignored; >4096 reply arrives as ordered chunks.
