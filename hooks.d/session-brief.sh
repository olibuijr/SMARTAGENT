#!/bin/sh
# Information-flow hook: put the live operating state in front of the agent at
# agent start (stdout = injected context). Recency beats instruction — the
# board/workflow/index state the agent must act on is shown, not described.
B=./target/release
seg() { "$@" 2>/dev/null | cut -d'|' -f2-; }

echo "SMARTAGENT operating state (live):"
echo "  board:    $(seg "$B/tasks" statusline --db data/tasks.semdb)"
doing="$("$B/tasks" list --col doing --db data/tasks.semdb 2>/dev/null)"
if [ -n "$doing" ] && [ "$doing" != "no tasks" ]; then
    printf '%s\n' "$doing" | head -n 3 | sed 's/^/  doing:    /'
fi
echo "  workflow: $(seg "$B/workflow" statusline --root . --db data/workflow.semdb)"
echo "  index:    $(seg "$B/codeindex" statusline)"
echo "Operating loop: see AGENT_TOOLS.md. File edits are hook-blocked while 'doing' is empty — capture and pull before touching files."
exit 0
