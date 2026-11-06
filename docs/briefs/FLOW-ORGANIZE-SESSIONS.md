---
id: FLOW-ORGANIZE-SESSIONS
type: flow
title: Arrange sessions in the rail
status: ready
references: [LAY-SHELL, PROTO-CORE-API]
adrs: [0026]
---

# Arrange sessions in the rail

## Outcome

The rail shows the user's sessions the way they want them: pinned ones first, then grouped and sorted as they chose, with archived sessions shown or hidden, and it is still that way after Ersa restarts.

## Trigger

The user opens a tile's menu (Pin, Rename, Move to section) or the arrange menu beside New session (Group by, Sort by, Show, New section, Reset to defaults, All sessions). `LAY-SHELL` (The rail) says where each control is and how it looks.

## Steps

1. On `LAY-SHELL`, the user chooses **Pin** in a tile's menu. The tile moves to the Pinned group at the top of the rail; its project and section are unchanged. **Unpin** returns it to its group.
2. The user chooses **Rename** in a tile's menu, or presses F2 on a tile with keyboard focus. The title becomes a field on the tile; Enter saves through the backend, as renaming in the transcript header does, and Escape restores it.
3. The user chooses **Move to section ▸** and one of: a section, "No section", or "New section…". "New section…" shows a new heading with an empty name field; Enter makes the section and moves the session into it, and Escape, or Enter on a blank name, makes nothing and leaves the session where it was.
4. From the arrange menu the user chooses **Group by** Project, Section, Date or None; **Sort by** Date created, Last activity or Name; and **Show** Active, Archived or All. The rail regroups at once, each tile sliding to its new place.
5. **New section** in the arrange menu shows a new heading with a name field, as in step 3, and makes an empty section when named. A section heading's More menu holds Rename and Delete.
6. The user collapses a group's heading. It stays collapsed in that grouping, and only that one.
7. **Reset to defaults** returns the grouping, sort, filter and collapsed groups to their defaults. Pins and sections stay.
8. Ersa restarts. The rail shows the same arrangement, pins and sections.
9. The user chooses **All sessions**. The transcript area shows the sessions page: every session in one list, whatever the rail's grouping, filter or collapsed groups, with Active and Archived tabs, a search field and New session. Choosing a session makes it the active session and the page gives way to its transcript; choosing a tile or New session in the rail leaves the page too.

## Variations and errors

- **Groups by date** are Today, Yesterday, Previous 7 days and Older, by when the session was created, in the user's local time, so a session never changes group as it works; only the passing of days moves it.
- **Sections in other groupings.** Sections are seen only when grouping by section. Choosing New section, from either menu, while grouping by something else switches the grouping to Section so the new heading is in view.
- **Deleting a section** puts its sessions in "No section" and asks nothing, since no session is lost.
- **A name already taken or too long.** Two sections may not share a name, ignoring case, and a name has at most 80 characters; the name field says so and keeps the name typed.
- **Saving fails.** When Core refuses or can't be reached, the change is undone in the rail and the rail says why. Nothing is kept only in the window.
- **A rename the backend refuses** (a blank or too-long title, or a record that is gone) keeps the field open with the reason. A session with no backend session id yet offers Rename as unavailable, with the reason; an untitled session that has one can be renamed.
- **Nothing to show.** Showing active sessions with every session archived, the rail says there are no active sessions. Changing what is shown never closes the active session, even when its tile leaves the rail.
- **A project whose sessions are all pinned** has its tiles in the Pinned group; its heading follows `LAY-SHELL` (Projects), and a collapsed heading counts only the tiles under it.
- **Last activity** moves a tile only when a turn ends, never while a reply streams, and never because the session's state changed.

## Requirements

- **FLOW-ORGANIZE-SESSIONS-R01**: When the user pins a session, Ersa shall show its tile in a Pinned group that leads the rail in every grouping, and shall not change its project or section; unpinning shall return it to its group.
- **FLOW-ORGANIZE-SESSIONS-R02**: When the user renames a session from its tile, Ersa shall rename it through `session.set_title`, the same as renaming from the transcript header.
- **FLOW-ORGANIZE-SESSIONS-R03**: When the user moves a session to a section or to "No section", Ersa shall store its membership in Core, and a session shall belong to at most one section.
- **FLOW-ORGANIZE-SESSIONS-R04**: When grouping by section, Ersa shall show every section, empty or not, in the order they were made, then "No section" for sessions in none.
- **FLOW-ORGANIZE-SESSIONS-R05**: When the user deletes a section, Ersa shall put its sessions in "No section" and keep every session.
- **FLOW-ORGANIZE-SESSIONS-R06**: Ersa shall refuse a section name that is blank, longer than 80 characters, or that another section already has, ignoring case.
- **FLOW-ORGANIZE-SESSIONS-R07**: When grouping by date, Ersa shall put each session in Today, Yesterday, Previous 7 days or Older by the local date it was created.
- **FLOW-ORGANIZE-SESSIONS-R08**: When grouping by project, Ersa shall group as `LAY-SHELL` (The rail) gives; when grouping by none, Ersa shall show one list with no headings after the Pinned group.
- **FLOW-ORGANIZE-SESSIONS-R09**: Ersa shall sort the tiles within each group by the chosen sort: date created newest first; last activity newest first, taking a session's creation for its last activity until it has one; or name in the user's locale order, with untitled sessions last; ties fall back to date created, newest first.
- **FLOW-ORGANIZE-SESSIONS-R10**: When sorting by last activity, Ersa shall move a tile only when Core reports a new last activity for its session, which Core sets when a turn ends.
- **FLOW-ORGANIZE-SESSIONS-R11**: When showing Active, Ersa shall leave archived sessions out of the rail; showing Archived, only archived sessions; showing All, both, each archived tile marked as archived.
- **FLOW-ORGANIZE-SESSIONS-R12**: When the user collapses or expands a group, Ersa shall keep that for the grouping in use, so the same group in another grouping is unaffected.
- **FLOW-ORGANIZE-SESSIONS-R13**: Pins, sections and their membership, the grouping, the sort, the filter and the collapsed groups shall survive a restart of Ersa and of Core.
- **FLOW-ORGANIZE-SESSIONS-R14**: When the user chooses Reset to defaults, Ersa shall group by project, sort by date created, show active sessions and expand every group, and shall keep pins and sections.
- **FLOW-ORGANIZE-SESSIONS-R16**: When the user chooses All sessions, Ersa shall show the sessions page in the transcript area, listing every session that is archived or not according to its tab (Active first), whatever the rail's grouping, filter and collapsed groups, newest first by last activity, taking a session's creation for its last activity until it has one.
- **FLOW-ORGANIZE-SESSIONS-R17**: While the user types in the sessions page's search field, Ersa shall list only the sessions whose title contains the text, ignoring case.
- **FLOW-ORGANIZE-SESSIONS-R18**: When the user chooses a session on the sessions page, Ersa shall make it the active session and show its transcript in place of the page.
- **FLOW-ORGANIZE-SESSIONS-R19**: While the sessions are being read, or when they cannot be read, the sessions page shall say so, with the reason when they cannot be read, and shall not say there are no sessions.
- **FLOW-ORGANIZE-SESSIONS-R20**: While the sessions page shows, Ersa shall hide the composer, the session controls and the transcript header, keep the active session active with its tile highlighted, and send nothing typed to it.
- **FLOW-ORGANIZE-SESSIONS-R15**: When Core refuses or fails a change to the arrangement, Ersa shall show the rail as Core has it and say why.

## Out of scope

- Reordering tiles or sections by dragging.
- Sections nested in sections, or a session in more than one.
- Arranging sessions on a remote host's Core differently from the local one.

## Open questions

- Whether pinned sessions keep their own order, set by the user, rather than the chosen sort.

## Changes

- 2026-11-05: from #174 merge review: the menu item is All sessions, not View all, so it doesn't read like Show ▸ All; R16's order takes creation for a session with no activity yet (reworded before it shipped); R19 and R20 added for the page's loading and failed states and what it hides.
- 2026-11-05: owner: View all opens the sessions page, as the desktop harness's does. R16 to R18 added.
- 2026-11-05: owner, before this shipped: date groups go by creation, so a session never changes group as it works. R07 reworded; no test cites it.
- 2026-11-05: created from the owner's decisions of 2026-11-05: pins, sections, grouping, sorting, filtering, collapsed groups kept across restarts (ADR 0026). R01 to R15 added.
