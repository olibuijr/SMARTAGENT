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
has_doing() {
    case "$1" in
        ""|"no tasks") return 1 ;;
        *) return 0 ;;
    esac
}

# Workspace-repo edit → that repo's own board satisfies the gate.
proj="$(printf '%s' "$payload" | sed -n 's/.*"path" *: *"[^"]*workspaces\/\([^/"]*\)\/.*/\1/p' | head -n 1)"
if [ -n "$proj" ]; then
    if has_doing "$("$TASKS" list --col doing --project "$proj" 2>/dev/null)"; then
        exit 0
    fi
fi
if has_doing "$("$TASKS" list --col doing --db data/tasks.semdb 2>/dev/null)"; then
    exit 0
fi

board_hint="tasks"
[ -n "$proj" ] && board_hint="tasks (project=$proj)"
cat >&2 <<EOF
kanban gate: nothing is in 'doing' — pull the work first, then retry this edit:
  $board_hint todo '<title>'            # capture (or: add '<title>' --criteria 'a;b')
  $board_hint move T-<n> doing          # pull it (WIP-limited)
Non-trivial (>=2 steps or >=2 files)? also run it through the engine:
  workflow start task-run --task T-<n>
(.scratch/ paths are exempt)
EOF
exit 2
