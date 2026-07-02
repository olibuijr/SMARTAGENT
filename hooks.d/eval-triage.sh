#!/bin/sh
# Stop hook — self-heal loop ingestion: sweep eval failures into deduped,
# criteria-gated board tasks the autonomous fleet pulls. Idempotent per
# sweep (open-task dedupe); recurring failures escalate to p1 instead of
# retrying. Fire-and-forget: output lands in the hooks audit row.
B=./target/release
"$B/evals" triage --db data/evals.jsonl --tasks-db data/tasks.semdb 2>&1 | tail -n 6
exit 0
