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

With no active session, or when the user chooses New session, the transcript area shows the new-session screen: the Ersa wordmark above the composer and nothing else, so the first message is the first act. Beneath the composer, on the new-session screen and on every session, sit the session controls: a row of chips for host, project, workspace (a new isolated workspace or the project's root workspace), backend, model, effort and permission mode, with the context ring at the row's end. Before a session starts the chips set it up; during a session they show its values. The backend chip appears when the host has more than one backend, and also when its only backend is missing or failed, so that status is seen (FLOW-NEW-SESSION-R07); with one available backend it is hidden. There is no setup card or form before the composer. The reference is the owner's sketch of 2026-11-02 (FLOW-NEW-SESSION).

The transcript header shows the active session's title. The title is renamed in place: a double-click on it, or Enter or F2 while it has focus, turns it into the rename field; Enter saves and Escape restores it. There is no rename button (owner, 2026-11-02).

## Development

A debug build attaches to a labelled Core endpoint, such as a replay, only when told to: `ERSA_ATTACH=<label>` makes it attach there and never launch a Core, and the Core tile reads "Attached to <label>". A release build ignores the variable and always uses the user's own Core.

## Open questions

- Whether the context ring takes a caution or danger tone near full, as the gauge vocabulary in DESIGN.md suggests, or stays one tone with the arc alone saying how much.
- Minimum window size, and which region collapses first when narrow.
- Whether the workbench shows one pane at a time (tabs) or allows splits.
- Whether the shelf overlays the transcript or pushes it, and how several pending shelves queue.
- Where global status (connected hosts, usage today) lives.
- Keyboard model: which shortcuts the shell owns versus the active pane.

## Changes

- 2026-11-02: the title is renamed in place by double-click or keyboard, with no button (owner); open question on the context ring's tone near full.
- 2026-11-02: from #53 inbox: the backend chip also shows a lone backend that is missing or failed.
- 2026-11-02: the new-session screen and the session controls beneath the composer, from the owner's sketch (FLOW-NEW-SESSION).
- 2026-11-01: from #14 inbox: a debug build attaches to a labelled endpoint when told to (Development).

- 2026-10-31: tile contents and shelf behaviour added from the owner's description; attention mark specified as edge or glyph.
- 2026-10-31: created as draft.
