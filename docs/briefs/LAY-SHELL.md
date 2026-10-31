---
id: LAY-SHELL
type: layout
title: Application shell
status: draft
references: []
adrs: [0008]
---

# Application shell

## Intent

The fixed structure of the Ersa window: a rail on the left listing projects, workspaces, and sessions; the transcript in the centre with the composer beneath it; and the workbench on the right holding purpose-built panes. The rail and workbench are collapsible and resizable; the transcript always has the most space. Sessions needing attention are visible in the rail from any state.

```
+--------+----------------------------+-----------+
|        |                            |           |
|  rail  |        transcript          | workbench |
|        |                            |           |
|        +----------------------------+           |
|        |         composer           |           |
+--------+----------------------------+-----------+
```

## Open questions

- Minimum window size, and which region collapses first when narrow.
- Whether the workbench can host more than one pane at once (tabs, split, or both).
- Where global status (connected hosts, total cost today) lives: rail footer or window title area.
- Keyboard model: which shortcuts the shell owns versus the active pane.

## Changes

- 2026-10-31: created as draft.
