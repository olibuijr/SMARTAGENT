#!/bin/sh
# Goal stop-hook (Claude Code /goal parity): after each turn an INDEPENDENT small
# model reads the session transcript and decides whether the active goal condition
# holds. No active goal → prints nothing (turn ends). Unmet → prints a hooks
# block-decision on stdout; the reason is surfaced as continuation guidance. Met →
# clears the goal (note on stderr). Never blocks the agent on its own errors.
exec ./target/release/goal check 2>/dev/null
