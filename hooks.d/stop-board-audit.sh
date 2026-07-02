#!/bin/sh
# Stop audit (slow balancing loop): snapshot board state at agent end so the
# hooks semdb audit trail records whether work ended with unfinished pulls.
# Fire-and-forget — output lands in the audit row, never blocks.
B=./target/release
# Surface gate bypasses: RELAX turns the kanban gate off for the whole run —
# that fact must land in the audit trail, not vanish.
[ "$SMARTAGENT_HOOKS_RELAX" = "1" ] && echo "NOTE: SMARTAGENT_HOOKS_RELAX=1 was active — kanban gate bypassed this session"
doing="$("$B/tasks" list --col doing --db data/tasks.semdb 2>/dev/null)"
if [ -n "$doing" ] && [ "$doing" != "no tasks" ]; then
    echo "session ended with tasks still in doing:"
    printf '%s\n' "$doing" | head -n 5
else
    echo "board clean at stop (nothing in doing)"
fi
exit 0
