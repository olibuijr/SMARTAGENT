#!/bin/sh
# Stop hook — self-heal loop ingestion: sweep eval failures into deduped,
# criteria-gated board tasks the autonomous fleet pulls. Idempotent per
# sweep (open-task dedupe); recurring failures escalate to p1 instead of
# retrying. Fire-and-forget: output lands in the hooks audit row.
set -eu
B=./target/release
DB=data/evals.semdb
TASKS=data/tasks.semdb
STAMP=data/eval-triage.last
MIN_SECS=60
now=$(date +%s)
mtime=0
if [ -f "$DB" ]; then
    mtime=$(stat -c %Y "$DB" 2>/dev/null || echo 0)
fi
if [ -f "$STAMP" ]; then
    # Format: epoch eval_db_mtime. If the eval DB has not changed and the last
    # sweep is younger than MIN_SECS, skip without invoking evals triage (and
    # therefore without replaying the full eval table on every turn-end).
    read -r last last_mtime < "$STAMP" || { last=0; last_mtime=0; }
    age=$((now - last))
    if [ "$last_mtime" = "$mtime" ] && [ "$age" -lt "$MIN_SECS" ]; then
        echo "evals triage: skipped (throttled ${age}s, eval db unchanged)"
        exit 0
    fi
fi
mkdir -p data
printf '%s %s\n' "$now" "$mtime" > "$STAMP"
"$B/evals" triage --db "$DB" --tasks-db "$TASKS" 2>&1 | tail -n 6
exit 0
