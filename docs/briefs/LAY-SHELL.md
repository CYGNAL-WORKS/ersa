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

The fixed structure of the Ersa window: a rail on the left, the transcript in the center with the composer beneath it, and the workbench on the right. The rail and workbench are collapsible and resizable; the transcript always has the most space.

```
+----------+----------------------------+-----------+
| rail     |                            |           |
|  project |        transcript          | workbench |
|   tile   |                            |  (panes)  |
|   tile  >|  shelf (pulls out from a   |           |
|  project |  tile; answer here)        |           |
|   tile   +----------------------------+           |
|          |         composer           |           |
+----------+----------------------------+-----------+
```

The rail lists projects and, under each, its sessions as tiles (The rail, below). When a session needs the user, its tile says so, and choosing the tile's chevron pulls a shelf out of it holding the request with its controls, so permissions and questions are handled without changing the active session (The shelf, below). The transcript shows the active session; the workbench holds Files, Diff, Terminal, Browser, and Developer panes.

With no active session, or when the user chooses New session, the transcript area shows the new-session screen: the Ersa wordmark above the composer and nothing else, so the first message is the first act. Above the composer's box, when there is one, sits a single line of status; `docs/design/COMPOSER.md` lists what it says and in what order. Beneath the box, on the new-session screen and on every session, sit the session controls in one row. On the left, where the session runs: host, project and workspace, each fixed once the session starts. On the right, how it runs: backend, model, effort and permission mode, with the context ring at the row's end. The host chip names the local host Local. The project chip's list shows each project's name, path, kind and trust, and ends with Add project; with no projects it reads Choose a folder. The workspace chip's list offers a new workspace or the project folder (the root workspace), and for a new workspace in a git project the base branch with a search box; the chip reads, for example, New workspace from main. Before a session starts the chips set it up; during a session they show its values, and the where chips name its workspace by its branch. The backend chip appears when the host has more than one backend, and also when its only backend is missing or failed, so that status is seen (FLOW-NEW-SESSION-R07); with one available backend it is hidden. There is no setup card or form before the composer. The owner-reviewed mockups are the Composer and place-picker boards; `docs/design/COMPOSER.md` says how the composer and the session controls look.

The transcript header shows the active session's title. The title is renamed in place: a double-click on it, or Enter or F2 while it has focus, turns it into the rename field; Enter saves and Escape restores it. There is no rename button (owner, 2026-11-02).

## The rail

From top to bottom: New session, the projects with their sessions, and the foot. The rail has no way to add a project: the new-session screen's project chip is the only one (FLOW-NEW-SESSION). The design documents say how each part and each state looks.

- **New session** opens the new-session screen.
- **Projects.** Each project is a project heading with its sessions beneath it as tiles, newest first; the projects are in the order Core lists them. A project with no sessions shows its heading alone. The heading names the project, and its host when the project is not on the local machine. Workspaces are not a level in the rail: the session controls name a session's workspace. A session whose project Ersa can't tell sits at the end of the tree under no heading, so a session is never missing from the rail.
- **Collapsing.** A heading collapses and expands its sessions, and stays as the user left it while the window is open. Collapsed, it says how many sessions it holds, with a mark when any of them needs the user.
- **A heading's actions** appear under the pointer or keyboard focus: New session, which opens the new-session screen for that project, and More, which holds trusting the project or withdrawing its trust. A project the user has not trusted says so on its heading, collapsed or not.
- **Tiles.** A tile shows the backend's icon, the session's title, a mark for its state, the context ring and the cost as reported. The cost appears once the backend has reported one, and the ring once it has reported both the tokens used and a context window larger than nothing; a session with no title yet is named as untitled. Choosing a tile makes its session the active one. The active session's tile is visibly distinct, quietly.
- **State.** A tile tells starting, working, error and disconnected apart by its mark, and names in words every state but working and idle. An idle or ended session has no mark. A state Ersa doesn't know has no mark either, and the tile says in words that Ersa doesn't know it.
- **Needs you.** A session with a pending permission request or question shows the shelf's handle in place of its state mark, in the tone of what it wants, and names the want: a permission request, a question, or how many requests when more than one is pending. With both kinds pending, the tone is the permission request's, and a request of a kind Ersa doesn't know reads as needing the user in that tone too. The pending requests decide this, whatever state Core reports: a session reported as waiting on the user with nothing pending reads as idle.
- **Unseen.** When Core reports a session that is not the active one as idle after working, the session is unseen, and its tile says so, until the user makes it active. Ersa keeps this while the window is open only.
- **Context.** The context ring is near full from 85 percent of the context window, on a tile and in the session controls alike, and never shows more than full.
- **Add project** sits at the end of the tree and opens the way to add one.
- **Loading, empty and failed.** While the projects are being read the tree says so; with no projects it says how to add one. When the projects, a project's workspaces or the sessions can't be read, the tree says so with the reason, and shows what it could read.
- **The foot** is empty while all is well, and while Ersa first connects, when the tree says it is loading. When Ersa can't reach Core, the foot says so in the user's terms, with the reason when Ersa has one, and the tree is empty. While Ersa is trying again the foot says that; once it has stopped trying, the foot offers Retry. Assistive technology is told when the problem appears and when Ersa reaches the sessions again. When Retry was pressed from the keyboard and the foot offers it again, keyboard focus returns to it unless the user has moved it. New session and Add project are unavailable until Ersa has connected. A debug build attached to a labeled endpoint names the label there (Development).
## The shelf

The shelf pulls out of the tile it belongs to, at the tile's height, and lies over the edge of the transcript: it never pushes the transcript aside, narrows it, or scrolls it. It is the width the design documents give it, and when it would run past the foot of the window it rises to fit. `FLOW-APPROVE-PERMISSION` says when it opens and closes, what it holds, and how it answers.

- **Loading, empty and failed.** While the projects are being read the tree says so; with no projects it says to choose a folder in New session. When the projects, a project's workspaces or the sessions can't be read, the tree says so with the reason, and shows what it could read.
- **The foot** is empty while all is well, and while Ersa first connects, when the tree says it is loading. When Ersa can't reach Core, the foot says so in the user's terms, with the reason when Ersa has one, and the tree is empty. While Ersa is trying again the foot says that; once it has stopped trying, the foot offers Retry. Assistive technology is told when the problem appears and when Ersa reaches the sessions again. When Retry was pressed from the keyboard and the foot offers it again, keyboard focus returns to it unless the user has moved it. New session is unavailable until Ersa has connected. A debug build attached to a labeled endpoint names the label there (Development).
## Development

A debug build attaches to a labeled Core endpoint, such as a replay, only when told to: `ERSA_ATTACH=<label>` makes it attach there and never launch a Core, and the rail's foot reads "Attached to <label>". A release build ignores the variable and always uses the user's own Core.

## Open questions

- Minimum window size, and which region collapses first when narrow.
- Whether the workbench shows one pane at a time (tabs) or allows splits.
- Where global status (connected hosts, usage today) lives, and where Core's version and API version are shown now that the rail's foot is silent while all is well.
- What a tile's More menu holds (Rename, Stop and Remove are drawn), and what Stop and Remove do to the session's workspace.
- Whether collapsed headings and unseen sessions are remembered when Ersa restarts, and whether a turn too short for a tile to show as working, or one that finishes while Ersa can't reach Core, should still leave its session unseen.
- Whether the tree keeps its tiles, marked as out of date, while Ersa can't reach Core, so a short drop doesn't empty the rail.
- Where the control that collapses the rail lives, and the one that shows it again.
- How a list that could not be read is read again while Core stays reachable. Today the lists are read when Ersa connects and after the user changes one.
- Where keyboard focus lands after a Retry in the foot that succeeds. The foot goes away while Ersa connects, so today focus falls to the window.
- How long assistive technology keeps the word that Ersa reconnected. Today it stays, unseen, until the next problem replaces it.
- Whether the foot shows that Ersa is trying after Retry is pressed. Today it goes away while Ersa connects and comes back unchanged when the try fails, so nothing says a try was made.
- How much of a failed start's own output the foot shows and assistive technology reads. Today the foot shows its last lines whole, which can push Retry out of sight, and they are read again each time a try fails.
- Keyboard model: which shortcuts the shell owns versus the active pane.
- Whether the user can reorder the rail's tiles or change its sort order; today it lists sessions newest first. DESIGN.md says how tiles move if so.

## Built so far

The rail is a list of projects above one list of sessions grouped under a heading per workspace. Of the rail described above, the foot is built and the rest is still to build. Until the shelf is built, a tile's chevron is a mark, not a control, and a request is answered in the transcript.

## Changes

- 2026-11-05: where the shelf sits (The shelf), from the owner's decisions; its behavior is FLOW-APPROVE-PERMISSION's. This closes the open question on overlay or push and queueing. No requirement IDs changed.

- 2026-11-05: the new-session screen and session controls from the owner's decisions: a status line above the composer's box; where chips on the left and how chips on the right; the project chip lists projects with path, kind and trust and ends with Add project; the workspace chip offers the base branch (FLOW-NEW-SESSION). No requirement IDs changed.
- 2026-11-05: from #155 merge review: whether the foot shows a try after Retry, and how much of a failed start's output it shows and reads, are open questions. No requirement IDs changed.
- 2026-11-05: from #155 inbox: the foot tells assistive technology when the problem appears and when it ends, and gives keyboard focus back to Retry (The foot); the open question on focus after Retry narrows to a retry that succeeds, and how long the word of reconnecting stays is a new one. No requirement IDs changed.
- 2026-11-05: Built so far follows the foot, now built; where keyboard focus lands after Retry is an open question. No requirement IDs changed.
- 2026-11-05: the rail specified from the owner-reviewed mockup (The rail): projects are headings with their sessions beneath and workspaces are not a level; a tile's contents and states; the shelf's handle on a session that needs the user; unseen sessions; what the tree says when a list can't be read; a foot that is silent while all is well. The context ring is near full from 85 percent, which closes that open question; FLOW-NEW-SESSION-R13 replaces R09 for where a new session's tile sits. No requirement IDs changed.
- 2026-11-02: the context-ring open question no longer points at DESIGN.md's gauge vocabulary, which ADR 0019 removed. Meaning unchanged; no requirement IDs changed.
- 2026-11-02: from #55 inbox: Built so far added: sessions grouped by workspace, the tree still to build. No requirement IDs changed.

- 2026-11-02: open question on reordering and sorting the rail, which DESIGN.md's motion rules anticipate. No requirement IDs changed.
- 2026-11-02: US spelling (ADR 0020, docs/design/CONTENT.md). Meaning unchanged; no requirement IDs changed.
- 2026-11-02: the title is renamed in place by double-click or keyboard, with no button (owner); open question on the context ring's tone near full.
- 2026-11-02: from #53 inbox: the backend chip also shows a lone backend that is missing or failed.
- 2026-11-02: the new-session screen and the session controls beneath the composer, from the owner's sketch (FLOW-NEW-SESSION).
- 2026-11-01: from #14 inbox: a debug build attaches to a labeled endpoint when told to (Development).

- 2026-10-31: tile contents and shelf behavior added from the owner's description; attention mark specified as edge or glyph.
- 2026-10-31: created as draft.
