---
name: ops
description: Keep the agent's services and safety rails running — supervise (a service is down or dead, restart the scheduler or chromium), schedule (cron job and one-shot reminder), notify (send a push notification), secrets (get a secret, credential, api key, token), sandbox (run a risky or untrusted command or script isolated), mcp (connect an external mcp server, call its tools), hooks (why an edit or write was blocked). USE WHEN service down, dead, restart, not responding, schedule a job, remind me, reminder, notification, alert me when done, secret, credential, api key, password, token, risky command, untrusted script, sandbox, mcp server, external tools, hook, edit blocked, gate.
---

# Ops — services, schedules, secrets, safety

## supervise — run `status` FIRST

When browser/search/schedule act dead, the cause is almost always a dead
service, not your call. `supervise status` before any debugging.

- Services: `scheduler` (fires cron jobs), `chromium` (headless CDP :9222).
- `up [service]`, `down [service]`, `restart <service>`,
  `logs <service>` (`tail`, default 40) for the why.
- State lives in data/supervise.semdb; the statusline ⛭ row mirrors it —
  red segment = act now.

## schedule — cron + one-shot reminders

- `add` with `notify='<message>'` and EITHER `cron='*/5 * * * *'` (recurring)
  OR `at='YYYY-MM-DDTHH:MM'` (one-shot, self-removes after firing).
- **Timezone gotcha:** `at` is local time only via `utc_offset_minutes` in
  config/smartagent.conf (default 0 = UTC). Check it before promising "at 9".
- `list`, `next` (upcoming fire times), `pause`/`resume`/`rm` (by `id`),
  `tick` (fire anything due right now — good for testing a job).
- Notify-reminders are the ONLY job type the agent can create; arbitrary
  shell `--cmd` is admin-gated. Jobs fire only while the scheduler service
  runs — nothing firing means `supervise status`, not a rewrite of the job.

## notify — push to the principal

- `send` needs `topic` + `message`; optional `title`, `priority` (1–5),
  `tags`, `click` (URL opened on tap), `markdown`.
- Use for: long run finished, error needing a human, requested reminders.
  Don't spam progress ticks.

## secrets — the ONLY path to credentials

- `get name=<X>` — caller-token authenticated (the launcher injects
  `SMARTAGENT_CALLER_TOKEN`; just call the tool). `set`, `list`, `audit`.
- Deny-by-default; every access audited. Granting access (policy-allow) is
  admin-only, out-of-band — if `get` is denied, tell the principal, don't
  work around it.
- **Never read a secret any other way** — not from env, not from files, not
  via sandbox tricks. If a command needs a credential, fetch via `secrets get`
  and pass it explicitly.

## sandbox — isolated exec for risky commands

- `run command='<cmd>'` — fresh workspaces/sandbox/<id>/ dir, env scrubbed,
  secrets tmpfs-masked, network OFF by default (`net=true` to allow),
  `timeout` default 30s, output capped 16KB.
- `tail=true` keeps the LAST bytes — right for build logs. `stdin=<file>`
  pipes input. `clean` prunes old sandbox dirs.
- Use for: anything web-sourced, anything destructive-looking, anything you
  wouldn't run in the repo cwd. A loud "isolation unavailable" warning means
  filesystem-only confinement — treat as weaker, not broken.

## mcp — external tool servers

- `tools` (`cmd='<stdio server command>'` or `url=<http>`, `auth_env` for a
  bearer token) — discover with `namesOnly`/`filter` first, schemas are big.
- `call` with `tool` + `args` (JSON); `head` caps the response.
- argv exec, no `sh -c` — the server command is not a shell line.

## hooks — the rails you'll run into (not a tool)

Deterministic lifecycle hooks (config/hooks.conf + hooks.d/) enforce the
operating loop:

- **require-doing-task** — edit/write BLOCKED while nothing is in `doing`
  (root board, or the workspace repo's own board). The block message contains
  the exact unlock commands (`tasks todo` → `tasks move T-n doing`). Exempt:
  `.scratch/`; `SMARTAGENT_HOOKS_RELAX=1` bypass exists but is audited — fix
  the board instead.
- **guard-destructive** — screens `sandbox` invocations.
- **session-brief** — injects live board/workflow/index state at agent start.
- **stop-board-audit** — snapshots the board into the audit trail at agent end.

`hooks list` / `hooks audit` / `hooks test <name>` (CLI) inspect and dry-run.
Hook failures fail OPEN with a warning — a wedged hook never wedges the agent.

## Gotchas

- Diagnosis order for "X is broken": `supervise status` → `supervise logs X`
  → only then the tool itself.
- `schedule tick` runs due jobs immediately — don't use it to "test" a job
  with real side effects (it will notify for real).
- Sandbox net is off by default when isolated: a curl inside will hang/fail
  by design — pass `net=true` deliberately, not reflexively.
