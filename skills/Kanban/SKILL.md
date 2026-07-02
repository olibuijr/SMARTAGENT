---
name: kanban
description: Kanban methodology rules for working the tasks board — pull-based flow, WIP limits, explicit blockers, criteria-gated done, flow metrics. USE WHEN managing tasks, planning work, deciding what to do next, triaging a backlog, running a standup or retro, or whenever the tasks tool is involved.
---

# Kanban — how to work the board

The `tasks` tool enforces the hard rules (WIP limits, criteria-gated done,
pull-based `next`) in Rust. This skill is the judgment layer on top: how a
kanban practitioner thinks. Workflows in `Workflows/` are runnable via the
`workflow` tool (`workflow start task-run`, `workflow start triage`, …).

## The six practices (Anderson kanban, adapted for an agent)

1. **Visualize** — the board IS the state. Start every work session with
   `tasks board`. Never hold task state in conversation memory only.
2. **Limit WIP** — doing=1 by default and that is deliberate: one thing at a
   time. When `tasks next` says WIP is full, FINISH the current task; never
   `--force` a second one in because it feels parallel-friendly.
3. **Manage flow** — watch `tasks metrics`. Rising cycle time means tasks are
   too big: split them. A task that sits in `doing` across sessions gets split
   or sent back to `ready` with a note.
4. **Explicit policies** — done means ALL criteria checked. Write criteria at
   add-time (`--criteria 'a;b;c'`), each one a binary check a tool can verify
   — the ISA/ISC discipline. A task without criteria is a wish, not work.
5. **Feedback loops** — run the `triage` workflow when the backlog grows past
   ~10, and the `retro` workflow after every few completed tasks.
6. **Improve** — retro learnings go to the memory tool (semantic tier), so the
   next session starts smarter.

## Column semantics

| Column | Means | Entry rule |
|---|---|---|
| backlog | captured, unrefined | anything (`tasks todo` is frictionless) |
| ready | refined: has prio + criteria, unblocked | refined during triage, never rots >2 weeks |
| doing | actively worked NOW | pulled via `tasks next`, WIP-limited |
| review | awaiting verification | criteria being checked one by one |
| done | all criteria verified | gate enforced by the tool |

## Blockers

A block always has a reason (`tasks block T-3 'waiting on titan reboot'`).
Blocked tasks are reviewed at every triage — a blocker older than a week is
either escalated (notify) or the task is rescoped so it isn't blocked.

## Priorities

p1 = blocks other work or the principal is waiting. p2 = normal. p3 = someday.
`tasks next` pulls by priority then age. Don't inflate: if everything is p1,
nothing is.

## Anti-patterns (never do these)

- Starting work that isn't on the board ("invisible WIP").
- `--force` past a WIP limit to feel productive — that's pushing, not pulling.
- Marking done with `--force` because the criteria "probably pass".
- Letting backlog items age without triage until the board lies.
