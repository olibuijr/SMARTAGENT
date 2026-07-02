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
2. **Ready beats backlog** — if any unblocked task is in `ready`, pull the
   highest-priority/oldest ready task with `tasks next`, then `tasks move T-n doing`.
3. **When ready is empty, promote exactly one backlog task** — choose by:
   p1 before p2 before p3, then oldest entry, then the smallest task id as a
   deterministic tie-breaker. Move only that task to `ready`, then pull it.
4. **Do not batch-promote** — keeping many tasks in `ready` makes ownership
   ambiguous for multi-agent work. One ready item is enough to feed the loop.
5. **Criteria gate before promotion** — if the chosen backlog task has no
   binary acceptance criteria, add or split criteria before moving it to `ready`.
6. **Review is handoff, not backlog** — tasks in `review` wait for the named
   assignee/role. Do not pull them unless the note/criteria address your role.

## Re-triage sweep

Run a sweep when priorities look stale or the user asks to re-triage:

1. `tasks board` to capture all open work.
2. Mark p1 only for tasks that block safety, operating-loop correctness,
   current principal directives, broken core tools, or cross-agent flow.
3. Mark p2 for normal product/quality work with clear criteria.
4. Mark p3 for cleanup, docs polish, nice-to-have UI, and superseded plans.
5. Close or merge duplicates instead of keeping competing backlog items.
6. Leave another agent's `doing`/`review` untouched; record a note or create a
   separate defect if coordination is needed.

## Heartbeat/autonomous rule

At beat time, if `doing` is empty and `ready` is empty, use this exact process:
`skills match 'triage backlog correct order'` → load `triage` → `tasks board` →
promote the single highest p/oldest backlog item to `ready` → `tasks next` →
move the returned task to `doing` → start `workflow task-run` for non-trivial work.

## Evidence to quote

- The `tasks board` lines showing `READY (0)` and the chosen backlog candidate.
- The `tasks move T-n ready`, `tasks next`, and `tasks move T-n doing` outputs.
- Any criteria added or priority changes made during a re-triage sweep.
