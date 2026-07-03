---
name: triage
description: Deterministic board triage and priority ordering for SMARTAGENT — decide correct order of business, promote backlog to ready, re-score open tasks, and handle heartbeat/autonomous backlog pulls. USE WHEN triage, re-triage, prioritize, priority, backlog, ready empty, correct order, what should I do next, order of business, promote backlog, stale tasks.
---

# Triage — correct order of business

Use this skill when the board needs a decision, not implementation. The goal is
reproducible pull order: two agents looking at the same board choose the same
next task without stealing work.

## Operating order

1. **Protect active work** — never take or move another agent's `doing` or
   `review` task. If more than one task is in your own `doing`, park extras
   back to `ready` before continuing.
2. **Sweep assigned review before new pulls** — if `review` contains a task
   assigned to your agent or role, verify it, move it with a note, or hand it
   off explicitly before triaging backlog. Other agents' review rows remain
   off-limits.
3. **Ready beats backlog** — if any unblocked, fleet-eligible task is in
   `ready`, pull the highest-priority/oldest ready task with `tasks next`, then
   `tasks move T-n doing`. Tasks tagged or titled `desktop-agent` are a separate
   session's WIP: never pull them for the gateway fleet, even if they appear in
   ready.
4. **When ready is empty, promote exactly one backlog task** — choose by:
   p1 before p2 before p3, then oldest entry, then the smallest task id as a
   deterministic tie-breaker. Move only that task to `ready`, then pull it.
5. **Do not batch-promote** — keeping many tasks in `ready` makes ownership
   ambiguous for multi-agent work. One ready item is enough to feed the loop.
6. **Criteria gate before promotion** — if the chosen backlog task has no
   binary acceptance criteria, add or split criteria before moving it to `ready`.
7. **Review is handoff, not backlog** — tasks in `review` wait for the named
   assignee/role. Do not pull them unless the note/criteria address your role.

## Re-triage sweep

Run a sweep when priorities look stale or the user asks to re-triage:

1. `tasks board` to capture all open work.
2. Sweep blockers first. A blocked row must be resolved by the dev team: unblock
   stale reasons, split/rescope, route off-lane/project-specific work to its
   project board, or create/pull the task that removes the blocker. Do not leave
   root-board blockers as implicit work for the principal.
3. Mark p1 only for tasks that block safety, operating-loop correctness,
   current principal directives, broken core tools, or cross-agent flow.
4. Mark p2 for normal product/quality work with clear criteria.
5. Mark p3 for cleanup, docs polish, nice-to-have UI, and superseded plans.
6. Close or merge duplicates instead of keeping competing backlog items.
7. Leave another agent's `doing`/`review` untouched; record a note or create a
   separate defect if coordination is needed.
8. Keep `desktop-agent` tasks out of fleet pull order: they belong on the
   concurrent desktop-agent lane/project board, not as lingering root blockers.

## Heartbeat/autonomous rule

At beat time, first check whether `review` has a row assigned to your agent or
role; if so, verify or hand it off. If `doing` is empty and `ready` is empty,
use this exact process: `skills match 'triage backlog correct order'` → load
`triage` → `tasks board` → promote the single highest p/oldest backlog item to
`ready` → `tasks next` → move the returned task to `doing` → start `workflow
task-run` for non-trivial work.

## Evidence to quote

- The `tasks board` lines showing `READY (0)` and the chosen backlog candidate.
- The `tasks move T-n ready`, `tasks next`, and `tasks move T-n doing` outputs.
- Any blockers cleared, routed to a project board, split/rescoped, or converted
  into an agent-owned follow-up task.
- Any criteria added or priority changes made during a re-triage sweep.
