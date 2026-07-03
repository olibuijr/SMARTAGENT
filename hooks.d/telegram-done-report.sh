#!/bin/sh
# Stop hook — broadcast a styled report to Telegram for tasks completed since
# the last check. Cursor = highest done id already reported (in .scratch).
# Report template: title, status, evidence bullets, next action.
# Fire-and-forget: output lands in the hooks audit row.
B=./target/release
CUR_FILE=.scratch/telegram-done-cursor
mkdir -p .scratch
if [ "$1" = "--sample" ]; then
    NEW='183	Telegram: richer notifications'
    MAX=183
else
LAST=$(cat "$CUR_FILE" 2>/dev/null || echo 0)
# Newest done tasks (id + title), tab-separated, done column.
NEW=$("$B/tasks" list --col done --db data/tasks.semdb 2>/dev/null | awk -F'\t' -v last="$LAST" '
    { gsub(/^T-/,"",$1); id=$1+0; if (id>last) { print id "\t" $3; if (id>max) max=id } }
    END { if (max>0) print "MAX\t" max > "/dev/stderr" }
' 2>.scratch/telegram-done-max)
MAX=$(awk -F'\t' '{print $2}' .scratch/telegram-done-max 2>/dev/null)
fi
[ -z "$NEW" ] && { echo "telegram-done: no new completions"; exit 0; }
COUNT=$(printf '%s\n' "$NEW" | grep -c .)
fmt_age() { s=$1; [ "$s" -lt 60 ] && { printf '%ss' "$s"; return; }; m=$((s/60)); [ "$m" -lt 60 ] && { printf '%sm' "$m"; return; }; printf '%sh%02dm' $((m/60)) $((m%60)); }
item_line() {
    id=$1 title=$2
    show=$("$B/tasks" show "T-$id" --db data/tasks.semdb 2>/dev/null || true)
    [ -z "$show" ] && show="T-$id [p?] done — $title"
    owner=$(printf '%s\n' "$show" | awk '/^owner:/{print $2; exit}')
    [ -z "$owner" ] && owner="unknown"
    crit=$(printf '%s\n' "$show" | awk 'BEGIN{d=0;t=0} /^  \[[ x]\]/{t++; if ($0 ~ /^  \[x\]/) d++} END{printf "%d/%d", d, t}')
    times=$(printf '%s\n' "$show" | awk '/→ doing @/ && first==0 {first=$3} /→ done @/ {done=$3} END{gsub("@","",first); gsub("@","",done); print first, done}')
    set -- $times; cycle="n/a"; [ -n "$1" ] && [ -n "$2" ] && [ "$2" -ge "$1" ] && cycle=$(fmt_age $(($2-$1)))
    first=$(printf '%s\n' "$show" | head -1)
    clean_title=$(printf '%s' "$first" | sed 's/^T-[0-9][0-9]* \[[^]]*\] [^—]* — //')
    [ -z "$clean_title" ] && clean_title=$title
    printf '• `T-%s` — %s\n  Owner: `%s` · Cycle: `%s` · Criteria: `%s`\n' "$id" "$clean_title" "$owner" "$cycle" "$crit"
}
ITEMS=$(printf '%s\n' "$NEW" | awk -F'\t' '{print $1 "\t" $2}' | head -10 | while IFS="	" read -r id title; do item_line "$id" "$title"; done)
FLEET=$("$B/tasks" metrics --db data/tasks.semdb 2>/dev/null | head -4 | sed ':a;N;$!ba;s/\n/; /g')
[ -z "$FLEET" ] && FLEET="metrics unavailable"
BODY="✅ *Agent report: completed work*

*Status:* $COUNT task(s) moved to done

*Evidence:*
$ITEMS
*Fleet:* $FLEET

*Next:* continue with the next unblocked ready task."
[ "$1" = "--print" ] || [ "$1" = "--sample" ] && { printf '%s\n' "$BODY"; exit 0; }
"$B/telegram" broadcast --text "$BODY" >/dev/null 2>&1 && echo "telegram-done: reported $COUNT completion(s)"
[ -n "$MAX" ] && echo "$MAX" > "$CUR_FILE"
exit 0
