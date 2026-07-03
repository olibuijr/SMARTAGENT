# T-289 Telegram reply verification

Date: 2026-07-03

## Root cause

Earlier Telegram listener behavior mixed two separate gates for group traffic:

1. `telegram_allowed_chats` is the hard security allow-list for chats the agent may answer.
2. `@<bot_username>` / reply-to-bot detection is only an addressing/noise filter inside an allowed group.

When those concerns were wrong, valid group messages could be ignored before they reached the gateway, while later changes also had to preserve the security property that an arbitrary unallowed group cannot gain agent access merely by mentioning the bot. The current code in `crates/telegram/src/main.rs` now rejects unallowed chats first, then requires an @mention or reply-to-bot only for allowed group/supergroup/channel messages, strips the mention before prompting the gateway, and preserves `message_thread_id` on outbound replies.

## Verification evidence

- `supervise status` shows `telegram` running.
- `supervise logs telegram` shows group chat `-1004429918544` receiving inbound messages and streamed replies with `status=ok`.
- `supervise logs telegram` shows direct chat `50020485` receiving inbound messages and streamed replies with `status=ok`.
