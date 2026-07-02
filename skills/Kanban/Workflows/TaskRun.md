---
name: task-run
description: Run one board task through the PAI-style phase loop — observe, plan, execute, verify, learn — with a designated skill per step
use_when: starting work on a pulled task; any task in doing
---

# Task Run

The five-phase loop for one task, PAI Algorithm style: each step names the
skill/tool to lean on, advancing requires evidence of what was actually done.
Start with the task linked: `workflow start task-run --task T-n`.

## observe
skill: memory
expect: relevant prior context + the task's criteria understood
Recall what's already known: `memory recall '<task topic>'`, `tasks show T-n`
for the criteria list. If the task touches code, `codeindex search` /
`codegraph defs` the affected area. If criteria are missing or vague, fix them
NOW (`tasks crit add`) — split any criterion that isn't a single binary check.

## plan
skill: tasks
expect: ordered sub-steps + which tool verifies each criterion
Decide the smallest path through the criteria. For each criterion name the
probe that will verify it (a command, a search, a request). If the task is
bigger than one session, split it on the board instead of carrying it.

## execute
skill: sandbox
expect: the work done, each criterion checked as it passes
Do the work. Untrusted or risky commands go through `sandbox run`. Check
criteria off as they ACTUALLY pass (`tasks crit check T-n <i>`), never in
batch at the end.

## verify
skill: evals
expect: every criterion re-verified with a live probe, evidence quoted
Re-run each criterion's probe fresh. Quote real output. If anything fails,
uncheck it and go back to execute. Log a trace (`evals log`) when the task has
a testable output worth regression-tracking.

## learn
skill: memory
expect: one durable learning stored, task moved to review/done
What would have made this task faster? Store it:
`memory remember '<learning>' --tier semantic`. Then `tasks move T-n review`
(or `done` if review isn't needed) and `tasks next` to pull the next one.
