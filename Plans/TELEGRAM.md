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

## Verify
`telegram send` delivers to Óli's chat (he sees it); `telegram poll` returns a message he sent; token only via secrets; non-allowlisted chat ignored; >4096 reply arrives as ordered chunks.
