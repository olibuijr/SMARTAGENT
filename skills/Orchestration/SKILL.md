---
name: orchestration
description: Parallelize and automate multi-step work — orchestrate (fan out N parallel headless pi subagents, subagent, delegate), workflow drive (engine-driven step loop, evidence-gated) vs workflow start (agent follows steps and advances), evals (score outputs, diff runs, catch a regression). Choosing between subagents and workflows. USE WHEN parallel agents, fan out, spawn subagents, delegate work, split work across agents, drive a workflow, start a workflow, advance a step, evidence, multi-step automation, process engine, evals, score a run, compare runs, regression.
---

# Orchestration — subagents, workflows, evidence

## Which mechanism

| Shape of the work | Use |
|---|---|
| N INDEPENDENT one-shot jobs (test each crate, summarize each file) | `orchestrate run` |
| Well-defined multi-step process, executed deterministically without you steering | `workflow drive` |
| Multi-step process YOU work through, step by step, with judgment between steps | `workflow start` + `advance` |
| One sequential task | no orchestration — just do it |

Fan-out buys wall-clock time only when jobs don't share files or state.
Dependent steps belong in a workflow, not in parallel agents.

## orchestrate — parallel headless subagents

- `run agents=N prompt='template with {i}'` — `{i}` substituted per agent
  (1..N). `timeout` per agent (default 300s), `max_parallel` caps the
  concurrent wave (default 4), `retries` re-runs failures.
- Each subagent is a headless `./pi` in its own `workspaces/<run-id>/agent-<n>/`
  dir, output to out.log. `list` shows runs; **`out runId=<id>` collects the
  output — a run isn't done until you've read it.**
- **Depth guard:** subagents run with SMARTAGENT_DEPTH+1 and fan-out beyond
  depth 1 is refused — a subagent that spawns subagents is a fork bomb, plan
  the split at the top level.
- Prompts must be self-contained: a subagent shares no conversation state
  with you. Tell it exactly what to produce and where.

## workflow — the process engine

- Discover: `list` (names, steps, USE WHEN), `show <name>` (step outline with
  per-step skills). Built-ins: `task-run` (observe→plan→execute→verify→learn),
  `triage`, `retro`, `status-report`. Definitions in workflows/*.md and
  skills/<name>/Workflows/.
- Agent-follows mode: `start <name> --task T-n` (link the board task) →
  `step` prints current instructions (load the skill it names) → do the work
  → `advance evidence='<what you verified and how>'`. Evidence is REQUIRED,
  ≥10 chars, and 'done'/'ok' are rejected — paste the probe result.
- Engine-driven mode: `drive <name> --task T-n` — the harness spawns a FRESH
  headless pi per step with ONLY that step's instruction, validates the
  step's final `EVIDENCE:` line in Rust, and advances or aborts. The model
  never self-advances. `--step-timeout`, `--retries` available.
- `runs` lists runs, `abort` kills one. `project=<repo>` keeps run state on
  that workspace repo — use the SAME project as the tasks board the run's
  task lives on.

## evals — score what came back

When fan-out or a driven run produces outputs whose QUALITY matters, don't
eyeball — log and score:

- `log run=<R> case=<id> input=… output=… [expected=…]` per case.
- `score run=<R>` (`matcher` exact|contains|regex-lite, `minPass` errors below
  a 0..1 threshold, `failOnly` to shrink output).
- `diff runA=<baseline> runB=<candidate>` for regressions; `runs` to browse.

## Gotchas

- **NEVER call `drive` from within a driven step** — nested drive is refused
  (SMARTAGENT_DRIVE=1) by design. If a step needs sub-work, it uses
  `orchestrate`, not another drive.
- `drive` holds the terminal for the whole run (one fresh pi per step) — for
  long runs prefer linking a `notify send` at the end, and never drive from
  inside orchestrated subagents.
- Evidence gaming is an anti-pattern the audit catches: evidence is the
  probe RESULT ("cargo test: 42 passed"), not a restatement of the step.
- Collect `orchestrate out` and spot-check at least one subagent's claim
  before reporting the fan-out as done (fusion-workflow rule: verify reports
  against reality).
