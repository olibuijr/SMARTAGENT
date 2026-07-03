# Telegram context model

T-149 defines the storage and prompt-context contract for Telegram conversations.
It is a design note for the follow-up implementation tasks (capture history,
inject context, and `/reset`/`/remember`).

## Scope keys

Every Telegram context row is scoped by four normalized keys:

| Key | Source | Required | Purpose |
|---|---|---:|---|
| `channel` | constant string `telegram` | yes | Prevents cross-channel mixing if other chat channels are added later. |
| `chat_id` | `message.chat.id` as a string | yes | Primary isolation boundary: a DM, group, or channel never sees another chat's context. |
| `thread_id` | `message.message_thread_id` as a string, else `main` | yes | Separates forum topics/threads inside one chat. DMs and non-topic chats use `main`. |
| `user_id` | `message.from.id` as a string, else `unknown` | yes for inbound user facts | Attributes speaker-specific facts without making them visible outside the chat/thread scope. |

Canonical scope id:

```text
telegram:<chat_id>:<thread_id>
```

Speaker-specific durable ids add the user key:

```text
telegram:<chat_id>:<thread_id>:user:<user_id>
```

The current allow-list still gates all inbound/outbound Telegram access before
context lookup. Context keys are identifiers, not authorization.

## Storage tiers and retention

Use two layers, both backed by SMARTAGENT-owned storage (no ad-hoc JSON files):

1. **Rolling session window** in `data/telegram.semdb`.
   - Rows represent inbound and outbound messages for one scope id.
   - Store direction (`in`/`out`), timestamp, Telegram `update_id` or sent
     `message_id`, `chat_id`, `thread_id`, `user_id`, and text.
   - Keep the most recent **40 turns or 24 hours per scope**, whichever is
     smaller for prompt injection.
   - Hard-retain raw rolling rows for **14 days** so `/reset` and debugging can
     remove or inspect recent context without growing forever.

2. **Durable memory** through the `memory` crate.
   - Only explicit user requests such as `/remember ...` or strong summaries
     promoted by an agent should enter durable semantic memory.
   - Durable facts include the scope id in the text/metadata so recall can be
     filtered to the same `channel/chat_id/thread_id` before injection.
   - Default recall budget: **up to 5 facts** for the current scope, plus at
     most **2 user-specific facts** for the current `user_id` when available.
   - No automatic global Telegram facts: cross-chat reuse requires an explicit
     principal-approved promotion outside this scoped model.

Prompt context order for replies:

1. System/platform instruction.
2. Current Telegram scope id and sender id.
3. Scoped durable facts.
4. Rolling transcript excerpt.
5. Current inbound message.

## Privacy, reset, and isolation

- **Per-chat isolation:** lookup and injection must filter by exact
  `channel=telegram`, `chat_id`, and `thread_id`. A group thread cannot read a
  DM; one forum topic cannot read another topic; non-Telegram channels cannot
  read Telegram context.
- **User attribution:** `user_id` refines who said or owns a fact, but it does
  not replace the chat/thread boundary. User facts are only injected inside the
  same chat/thread unless explicitly promoted by a human-approved workflow.
- **`/reset`:** deletes the rolling session rows for the current canonical
  scope id and suppresses durable fact injection for that scope until the next
  message. It must not delete other chats, other thread ids, or global agent
  memory.
- **`/reset all`:** optional admin-only behavior for a future task; if added,
  it may delete all Telegram rows for the same `chat_id` but still must not
  cross into other chats.
- **Durable deletion:** `/reset` does not erase durable `/remember` facts. A
  separate explicit command (for example `/forget <id>`) or human board task is
  required to remove durable memory.
- **Visibility:** `/commands` or `/help` should mention that recent Telegram
  chat context is used for replies and that `/reset` clears the current chat or
  thread window.

## Acceptance probes for implementation tasks

- Send messages in two fake scopes (`telegram:chat-a:main` and
  `telegram:chat-b:main`) and verify each injected transcript contains only its
  own scope.
- Send two thread ids in one chat and verify `/reset` in one thread leaves the
  other thread intact.
- Store a `/remember` fact and verify normal `/reset` clears rolling context but
  does not delete the durable fact.
