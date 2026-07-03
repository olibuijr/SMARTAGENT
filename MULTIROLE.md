# MULTIROLE.md — GOAGENT-style roles and handoffs for ./pi

## Model

GOAGENT-style multirole work means one visible kanban board, several focused agents, and explicit handoffs. A role owns a narrow kind of work, moves exactly one task at a time, and leaves evidence on the board before another role picks it up.

The board is the handoff protocol:

1. **backlog** — captured, not ready for a role.
2. **ready** — criteria are clear; any matching role may pull it.
3. **doing** — one agent is actively working it. Do not take tasks already here.
4. **review** — work is ready for a different role to verify.
5. **done** — criteria are checked with evidence.

## Roles

| Role | Pulls | Uses | Handoff rule |
|---|---|---|---|
| Planner | vague backlog items | `tasks`, `workflow`, `skills` | Adds binary criteria, then moves to `ready`. |
| Builder | implementation tasks | `codeindex`, `codegraph`, `edit`, `bash` | Moves to `review` when tests/builds pass. |
| Tester | QA and regression tasks | `bash`, `sandbox`, `browser`, `evals` | Checks criteria or sends defects back to `backlog`. |
| Researcher | docs/web/reference tasks | `rag`, `search`, `browser`, `vault` | Produces cited notes or a concrete implementation task. |
| Ops | services/secrets/schedules | `supervise`, `secrets`, `schedule`, `notify` | Leaves service health evidence and blocks if human credentials are needed. |
| Coordinator | flow hygiene | `tasks board`, `tasks metrics`, `workflow runs` | Resolves stale `doing`/`review`, never edits code while coordinating. |

## SMARTAGENT/pi mapping

- `skills match` routes every role to the right operating mode before work starts.
- `tasks` is the shared queue and lock: if a task is in `doing`, another agent does not touch it.
- `workflow start task-run` gives non-trivial work the observe → plan → execute → verify → learn loop.
- `orchestrate` is for independent fan-out, not for multiple agents editing the same task.
- `memory` stores durable project facts; role-local scratch stays out of memory unless it affects future work.
- `vault` holds human-readable design notes; implementation state belongs on the board.
- `codeindex`/`codegraph` are the default code-navigation tools before editing.
- `supervise`, `schedule`, `secrets`, and `notify` are Ops tools and should not be bypassed with ad-hoc shell access.

## Standards

### TDD / verification

- Define acceptance criteria before work enters `ready`.
- Prefer the smallest failing probe first: unit test, CLI command, browser probe, or eval case.
- A task is done only after the real probe passes and each criterion is checked.
- Bug fixes target the shared root cause, not just the reported path.

### Dev-team discipline

- One task per agent in `doing`.
- Do not edit files for unclaimed work.
- If another agent is already working a task, choose a different `ready` task or coordinate in `review`.
- Keep diffs small and role-scoped; split work that crosses unrelated areas.

### Ops discipline

- Use `supervise status` before restarting services.
- Use `secrets get` only through the policy-gated tool.
- Use `sandbox` for risky or untrusted commands.
- Never write scratch or generated data outside the repo; use `.scratch/` or the relevant `workspaces/<project>/` store.

## Implementation plan

1. Keep this document as the role contract for ./pi multirole sessions.
2. Add role-specific skills only when a repeated workflow needs more detail than this table.
3. Extend gateway multi-agent work so named agents own their own session IDs and advertise role + current task.
4. Teach Coordinator heartbeats to flag stale `doing` tasks and tasks in `review` without evidence.
5. Add smoke tests that prove two agents cannot claim the same board task without an explicit handoff.
