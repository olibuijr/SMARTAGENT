---
name: status-report
description: Two-step engine-driven smoke — inspect the board, then confirm the workspace index
use_when: verifying the drive engine; producing a quick board+index status
---

# Status Report

Minimal two-step workflow, safe to drive: read-only inspection of the board
and the workspace index.

## board
skill: tasks
expect: current board summary reported
Run the tasks tool with action board and summarize the columns in one line.

## index
skill: codeindex
expect: workspace index coverage reported
Run the codeindex tool with action projects and report how many repos are
indexed out of how many.
