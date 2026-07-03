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
   Criteria that begin with `cargo test`, `cargo build`, or `cargo check` are
   not just trusted as text: `tasks done` re-runs them in the task worktree
   with a timeout and refuses closure if they fail or hang.
5. **Feedback loops** — load the `triage` skill for backlog ordering or
   priority sweeps, run the `triage` workflow when the backlog grows past ~10,
   and the `retro` workflow after every few completed tasks.
6. **Improve** — retro learnings go to the memory tool (semantic tier), so the
   next session starts smarter.

## Column semantics

| Column | Means | Entry rule |
|---|---|---|
| backlog | captured, unrefined | anything (`tasks todo` is frictionless) |
| ready | refined: has prio + criteria, unblocked, fleet-eligible | refined during triage, never rots >2 weeks |
| doing | actively worked NOW | pulled via `tasks next`, WIP-limited |
| review | awaiting verification | named assignee/role verifies or re-hands off with a note |
| done | all criteria verified | gate enforced by the tool; checked `cargo test`, `cargo build`, and `cargo check` criteria are re-run by `tasks done` in the task worktree with a timeout |

## Reviewer flow

Review is active handoff, not a parking lot. On each beat, an agent checks
review rows assigned to its exact owner marker (for example `@grace`) or role
marker (for example `@QA`) before pulling new ready/backlog work. The reviewer
either verifies criteria and moves the task forward, or leaves an explicit note
and reassigns/hands it off. Review rows owned by another agent or role are
protected WIP and must not be pulled opportunistically.

## Blockers

A block always has a reason (`tasks block T-3 'waiting on titan reboot'`).
Blocked tasks are reviewed at every triage/beat and must not linger. The dev
team owns resolution: unblock obsolete reasons, split/rescope the task so work
can proceed, move off-lane/project-specific work to its owning project board, or
create and pull an actionable dev-team task to remove the blocker. Do not leave
a blocked task waiting for the principal unless the task states the concrete
agent-owned next step and owner. Root-board blockers should trend to zero.

## Priorities

p1 = blocks other work or the principal is waiting. p2 = normal. p3 = someday.
`tasks next` pulls by priority then age, excluding `desktop-agent` work from
this gateway fleet lane. Don't inflate: if everything is p1,
nothing is.

## Anti-patterns (never do these)

- Starting work that isn't on the board ("invisible WIP").
- `--force` past a WIP limit to feel productive — that's pushing, not pulling.
- Marking done with `--force` because the criteria "probably pass".
- Letting backlog items age without triage until the board lies.
- Letting blocked tasks linger as a parking lot or as implicit human homework.
- Inventing ad-hoc pull order; use the `triage` skill when `ready` is empty or priorities are disputed.
- Gateway agents restarting their own host service in two steps. A T-79-style
  verification that needs `gateway` restarted must either use one atomic
  `supervise restart gateway` as its last action, or move the task to review /
  hand off to a peer-orchestrated verifier that can complete restart+probe
  outside the service being restarted.
