#!/bin/sh
# Kanban gate (balancing loop): file mutations require pulled work-in-progress.
# Fires on pi's edit/write tool_calls. Allow iff a task is in `doing` on the
# relevant board — the workspace repo's own board when the target path is
# under workspaces/<project>/, else the root board. The block message IS the
# corrective command sequence (load-bearing: small models act on what's in
# front of them, not prose).
# Exempt: .scratch/ throwaways; SMARTAGENT_HOOKS_RELAX=1 (headless test runs).
[ "$SMARTAGENT_HOOKS_RELAX" = "1" ] && exit 0
payload="$(cat)"
case "$payload" in
    *'.scratch/'*) exit 0 ;;
esac

TASKS=./target/release/tasks
is_eval_expectation_path() {
    case "$payload" in
        *'"path"'*'data/evals.semdb'*|*'"path"'*'Testing/runs/'*|*'"path"'*'Testing/TESTPLAN.md'*) return 0 ;;
        *) return 1 ;;
    esac
}
block_eval_expectation_weakening() {
    doing="$1"
    if is_eval_expectation_path && printf '%s' "$doing" | grep -q 'eval-fix'; then
        if printf '%s' "$doing" | grep -vq 'eval-fix'; then
            return 0
        fi
        cat >&2 <<EOF
 eval expectation gate: eval-fix remediation may not edit eval expectations/results.
 Fix the product code/config and re-run the real probe; do not weaken expected
 outputs, matchers, or run evidence while an eval-fix task is in doing.
EOF
        exit 2
    fi
}
has_doing() {
    case "$1" in
        ""|"no tasks") return 1 ;;
        *) return 0 ;;
    esac
}
first_task_id() {
    printf '%s' "$1" | sed -n 's/^\(T-[0-9][0-9]*\)[[:space:]].*/\1/p' | head -n 1
}
json_path() {
    printf '%s' "$payload" | sed -n 's/.*"path" *: *"\([^"]*\)".*/\1/p' | head -n 1
}
require_current_worktree_path() {
    tid="$1"
    p="$(json_path)"
    [ -z "$tid" ] && return 0
    [ -z "$p" ] && return 0
    [ ! -d "worktrees/$tid" ] && return 0
    case "$p" in
        worktrees/"$tid"/*|*/worktrees/"$tid"/*) return 0 ;;
    esac
    cat >&2 <<EOF
worktree isolation gate: task $tid has an isolated worktree at worktrees/$tid.
Edit files inside that worktree, not the shared root checkout:
  cd worktrees/$tid
EOF
    exit 2
}

# Workspace-repo edit → that repo's own board satisfies the gate.
proj="$(printf '%s' "$payload" | sed -n 's/.*"path" *: *"[^"]*workspaces\/\([^/"]*\)\/.*/\1/p' | head -n 1)"
if [ -n "$proj" ]; then
    doing_list="$("$TASKS" list --col doing --project "$proj" 2>/dev/null)"
    block_eval_expectation_weakening "$doing_list"
    if has_doing "$doing_list"; then
        require_current_worktree_path "$(first_task_id "$doing_list")"
        exit 0
    fi
    board_hint="tasks (project=$proj)"
else
    doing_list="$("$TASKS" list --col doing --db data/tasks.semdb 2>/dev/null)"
    block_eval_expectation_weakening "$doing_list"
    if has_doing "$doing_list"; then
        require_current_worktree_path "$(first_task_id "$doing_list")"
        exit 0
    fi
    board_hint="tasks"
fi
cat >&2 <<EOF
kanban gate: nothing is in 'doing' — pull the work first, then retry this edit:
  $board_hint todo '<title>'            # capture (or: add '<title>' --criteria 'a;b')
  $board_hint move T-<n> doing          # pull it (WIP-limited)
Non-trivial (>=2 steps or >=2 files)? also run it through the engine:
  workflow start task-run --task T-<n>
(.scratch/ paths are exempt)
EOF
exit 2
