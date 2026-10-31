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

The fixed structure of the Ersa window: a rail on the left, the transcript in the centre with the composer beneath it, and the workbench on the right. The rail and workbench are collapsible and resizable; the transcript always has the most space.

```
+----------+----------------------------+-----------+
| rail     |                            |           |
|  project |        transcript          | workbench |
|   ws     |                            |  (panes)  |
|    tile  |                            |           |
|    tile >|  shelf (pulls out from a   |           |
|   ws     |  tile; answer here)        |           |
|    tile  +----------------------------+           |
|          |         composer           |           |
+----------+----------------------------+-----------+
```

The rail lists projects, each project's workspaces, and each workspace's sessions as tiles. A tile shows title, backend icon, activity state, a context ring, and cost as reported. When a session needs the user, its tile marks that (an edge or glyph, never a filled row) and a shelf pulls out from it holding the request with its controls, so permissions and questions are handled without changing the active session. The transcript shows the active session; the workbench holds Files, Diff, Terminal, Browser, and Developer panes.

## Open questions

- Minimum window size, and which region collapses first when narrow.
- Whether the workbench shows one pane at a time (tabs) or allows splits.
- Whether the shelf overlays the transcript or pushes it, and how several pending shelves queue.
- Where global status (connected hosts, usage today) lives.
- Keyboard model: which shortcuts the shell owns versus the active pane.

## Changes

- 2026-10-31: tile contents and shelf behaviour added from the owner's description; attention mark specified as edge or glyph.
- 2026-10-31: created as draft.
