---
name: triage
description: Refine the backlog — dedupe, prioritize, write criteria, promote to ready, age out stale items
use_when: backlog >10 items, board feels stale, start of a work session after time away
---

# Triage

Backlog hygiene. Run when the backlog grows past ~10 or the board has rotted.

## survey
skill: kanban
expect: full picture of backlog + blocked items
`tasks board`, `tasks list --col backlog`, `tasks list --blocked`. Note
duplicates, stale items (created long ago, never moved), and blockers older
than a week.

## refine
skill: kanban
expect: every kept backlog item has prio + ≥1 binary criterion
For each keeper: set real priority, add criteria (`tasks crit add`), merge
duplicates (`tasks rm` the copy). Items nobody will ever do: `tasks rm` —
a graveyard backlog hides the real work. Escalate week-old blockers
(`notify send`) or rescope the task so it isn't blocked.

## promote
skill: kanban
expect: ready column holds the true next-up work, nothing more
Move the refined top items to ready (`tasks move T-n ready`). Ready is a
commitment queue, not a second backlog — keep it ≤5.
