#!/bin/sh
# Stop hook — broadcast a styled report to Telegram for tasks completed since
# the last check. Cursor = highest done id already reported (in .scratch).
# Fire-and-forget: output lands in the hooks audit row.
B=./target/release
CUR_FILE=.scratch/telegram-done-cursor
mkdir -p .scratch
LAST=$(cat "$CUR_FILE" 2>/dev/null || echo 0)
# Newest done tasks (id + title), tab-separated, done column.
NEW=$("$B/tasks" list --col done --db data/tasks.semdb 2>/dev/null | awk -F'\t' -v last="$LAST" '
    { gsub(/^T-/,"",$1); id=$1+0; if (id>last) { print id "\t" $3; if (id>max) max=id } }
    END { if (max>0) print "MAX\t" max > "/dev/stderr" }
' 2>.scratch/telegram-done-max)
MAX=$(awk -F'\t' '{print $2}' .scratch/telegram-done-max 2>/dev/null)
[ -z "$NEW" ] && { echo "telegram-done: no new completions"; exit 0; }
COUNT=$(printf '%s\n' "$NEW" | grep -c .)
BODY="✅ *$COUNT task(s) completed*
$(printf '%s\n' "$NEW" | sed 's/^\([0-9]*\)\t/• T-\1 — /' | head -10)"
"$B/telegram" broadcast --text "$BODY" >/dev/null 2>&1 && echo "telegram-done: reported $COUNT completion(s)"
[ -n "$MAX" ] && echo "$MAX" > "$CUR_FILE"
exit 0
