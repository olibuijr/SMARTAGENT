# Telegram response templates

Purpose: keep Telegram replies readable in a narrow mobile chat while preserving enough structure for status, board, memory, and workflow use. These templates cover the response types emitted by `crates/telegram/src/main.rs` and by agent replies streamed through the gateway.

## Inventory of response types

| Type | Source | Current trigger | Template |
|---|---|---|---|
| Command catalog | `command_help` | `/help`, `/commands`, `/start` | Help catalog |
| Board snapshot | `tasks board` via `slash_command` | `/board` | Board/status block |
| Ready task list | `tasks list --col ready` via `slash_command` | `/tasks` | Task list |
| Service status | `supervise status` via `slash_command` | `/status` | Status checklist |
| Agent list | `gateway agents` via `slash_command` | `/agents` | Status checklist |
| Workflow runs | `workflow runs --live` via `slash_command` | `/runs` | Table when compact, bullets when long |
| Skill discovery | `skills list` / `skills match` via `slash_command` | `/skills [query]` | Bullet list with match scores when present |
| Memory recall | `memory recall` via `slash_command` and scoped context | `/memory query` | Evidence bullets |
| Memory write | `remember_context_fact` | `/remember fact` | Confirmation |
| Context reset | `reset_context` | `/reset` | Confirmation |
| Model menu / selection | `model_menu_text`, `set_model_preference` | `/model`, `/model N`, inline callback | Menu or confirmation |
| Stop/cancel | `stop_context`, `stream_reply` cancel branch | `/stop` | Confirmation/error |
| Authorization denial | `authorize_slash_command` | any protected slash command | Error/blocker |
| Slash command errors | `slash_command` command runner | CLI spawn/failure | Error/blocker |
| Agent streaming placeholder | `stream_text_response`, `stream_reply` | slash commands and normal chat | Progress frame |
| Tool progress | `telegram_tool_status`, `ProgressEvent::ToolUse` | gateway stream info | Progress frame |
| Planning/waiting/verifying progress | `ProgressEvent` | streaming normal chat | Progress frame |
| Final agent answer | `finish_streamed_message` | normal chat completion | Agent answer |
| Task creation / board mutations mentioned by agent | agent replies, tasks tool output | natural-language agent work | Task mutation |
| Blockers | task board output or agent replies | blocked tasks / denied actions | Blocker |
| Empty output | `stream_text_response`, command runner | blank CLI/agent result | Empty result |

## Base rules

- Start with a short human title line, not a Markdown `#` heading.
- Use emoji only as a fast status cue; keep the text meaningful without emoji.
- Prefer bullets for mobile readability.
- Keep each frame useful even if it is the only frame the user sees.
- Preserve raw command output when exact evidence matters, but wrap it with a concise summary.
- For streaming previews, show at most one current status line plus the newest useful body text.

## Templates

### Help catalog

```text
SMARTAGENT Telegram commands:
• /help — Show SMARTAGENT Telegram commands
• /board — Show the kanban board
• /tasks — List ready tasks
...
```

Use the registered command descriptions as the source of truth; the help body must match the Bot API command menu.

### Board/status block

```text
📋 Board
Status: <overall state>

Ready
• T-123 p1 <title> [1/3✓]
• T-124 p2 <title>

Doing
• T-125 p1 <title> @owner [2/3✓]

Blocked
• T-126 — <block reason>
```

Use a table only when the board is small enough to fit without wrapping; otherwise use columns as headings with bullets.

### Task list

```text
🧾 Ready tasks
• T-123 p1 — <title> [criteria]
• T-124 p2 — <title>

Next: use /board for full WIP and blockers.
```

### Task mutation / task creation

```text
✅ Task updated
• Task: T-123 — <title>
• Change: moved to doing / criterion 2 checked / created in ready
• Evidence: <short probe result>
```

Use this for agent reports that create, move, block, unblock, or complete tasks.

### Blocker

```text
⛔ Blocked
• Task: T-123 — <title>
• Reason: <specific reason>
• Next: <who/what is needed>
```

For permission denials, replace `Task` with `Command`.

### Status checklist

```text
🩺 Status
• scheduler: OK
• gateway: OK
• chromium: down — <action or impact>

Next: <only if action is needed>
```

Use bullets rather than a table because service names and health details wrap unpredictably on phones.

### Workflow runs

For short run lists:

```text
🏃 Active runs
| Run | Task | Step | State |
|---|---|---:|---|
| W-174 | T-173 | 4/5 | verify |
```

For long run lists:

```text
🏃 Active runs
• W-174 — T-173, step 4/5 verify
• W-173 — T-166, step 3/5 execute
```

### Memory recall

```text
🧠 Memory
Query: <query>

• <score> — <fact/source>
• <score> — <fact/source>

No matching memory found.
```

Do not mix chat/thread scoped memories across Telegram scopes.

### Confirmation

```text
✅ Done
• <what changed>
```

Examples: remembered a fact, reset context, selected a model, stopped a response.

### Error

```text
⚠️ Could not complete <command/action>
• Reason: <plain-language error>
• Try: <next safe action, if any>
```

Never expose secret values or raw tokens in errors.

### Progress frame

```text
🧭 Planning the next step…

<latest visible answer text or short status>
```

Allowed status cues are the existing progress events:

- `🧭 Planning the next step…`
- `🔧 Using tools…`
- `⏳ Waiting for a result…`
- `✅ Verifying before replying…`
- `💬 Final answer ready.`

Tool status lines may append sanitized `🛠 <tool> …` details. Do not show hidden reasoning or chain-of-thought.

### Agent final answer

```text
<title or direct answer>

Status: <done / partial / blocked>
Evidence:
• <probe result>
• <file or task id>
Next:
• <only if relevant>
```

For very short Q&A, a direct plain sentence is better than the full report template.

### Empty result

```text
(no output)
```

Use only when the underlying command truly returned no body.

## Plain text vs Markdown tables

Use plain text bullets when:

- More than 4 rows are expected.
- Any cell may contain long task titles, paths, error text, or blocker reasons.
- The message is a streaming preview that may be edited repeatedly.
- Telegram wrapping would make columns harder to scan.

Use Markdown tables when:

- There are 2–4 columns with short stable values, such as run id, task id, step, and state.
- The table has at most about 8 rows.
- The final message is sent with Markdown enabled and has been clipped to Telegram's 4096-character cap.

Avoid tables in live progress frames; use bullets or a single status line instead.
