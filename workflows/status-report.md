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
Run the tasks tool with action board. Report with this template:

```
Agent report: status
- Status: <doing/ready/review counts>
- Evidence: <one board line proving the counts>
- Next: <single next action>
```

## index
skill: codeindex
expect: workspace index coverage reported
Run the codeindex tool with action projects. Report with this template:

```
Agent report: workspace index
- Status: <indexed repos>/<total repos> indexed
- Evidence: <one projects summary line>
- Next: <single next action, or "none">
```
