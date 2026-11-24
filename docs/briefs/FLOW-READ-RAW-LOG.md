---
id: FLOW-READ-RAW-LOG
type: flow
title: Read a session's raw log in the developer pane
status: ready
references: [LAY-SHELL, PROTO-CORE-API]
adrs: [0017]
---

# Read a session's raw log in the developer pane

## Outcome

Someone working on Ersa, or finding out why a session misbehaved, sees exactly what Ersa and the session's backend said to each other, as it happens, without leaving the window or stopping the session. Nobody else ever needs the pane: it is closed until asked for, and nothing a person does in Ersa depends on it.

Claude Desktop has no such pane, so nothing here follows it. The pane reads as the network list of a browser's developer tools does, which the people who open it already know.

## Trigger

The user chooses Developer pane in the arrange menu (`LAY-SHELL`, The workbench).

## Steps

1. The workbench opens at the right of the window with the developer pane in it, and the menu item is marked. Choosing the item again, or the pane's Close, closes the workbench, and keyboard focus that was inside it goes to the arrange button. Ersa remembers whether it was open, and how wide, the next time it starts.
2. The pane lists the active session's raw log, oldest line first: when the line crossed, which way it went (from the backend, or to it), and the line's start, one line to a row.
3. While the session runs, new lines join the end and the list follows them. Once the user scrolls away from the end, the list stays where they are reading, says how many lines have arrived since, and offers Jump to latest.
4. The user selects a row. Beneath the list the pane shows that line whole, laid out with indentation when it is JSON, with Copy, which copies the line as it crossed.
5. The user types in the filter field. The list shows only the lines that contain the text, whatever its case, and says how many of the lines those are.
6. The user chooses Open diagnostic log. The diagnostic log's file opens in the app the system uses for it.
7. The user makes another session active. The pane shows that session's raw log, with no row selected; the filter stays.

## Variations and errors

- **No active session** (the new-session screen): the pane says to choose a session.
- **Nothing held**: a session Ersa is not running, or one that ran while nobody had it open, has no lines. The pane says they appear while the session runs.
- **The backend starts again** (a resume, or a start after a failure): the earlier lines stay and the new ones follow them (PROTO-CORE-API-R136, R137).
- **Earlier lines are gone.** Core holds a bounded amount of each session's raw log, and the pane holds the newest 50,000 lines. When lines before the first one listed are no longer held, the list's first row says so.
- **A very long line**, such as an attached image: the row shows its start; the detail shows the start Core sent, says how large the whole line is, and Copy copies what is shown.
- **The raw log can't be read**, as when Ersa can't reach Core: the pane says so with the reason, keeps the lines it has, and keeps trying; when a read succeeds the message goes.
- **Ersa connects to Core again.** A Core that restarted holds none of the lines it had and numbers them from the start again, so the pane lets go of the lines it holds and reads the session's raw log from the first line held. Against the same Core that reads the same lines again.
- **The diagnostic log can't be opened**: the pane says so with the reason. In a debug build attached to a labeled endpoint (`LAY-SHELL`, Development) Ersa has no diagnostic log of its own, and the button is unavailable and says why.
- **The open state can't be kept**, as against a replay, which keeps no settings: the workbench opens and closes all the same, and nothing is said.
- **The sessions page or the settings page is showing**: the workbench stays, showing the active session's raw log.

## Requirements

- **FLOW-READ-RAW-LOG-R01**: When the user chooses Developer pane in the arrange menu while the workbench is closed, Ersa shall open the workbench with the developer pane; when they choose it while the workbench is open, or choose the pane's Close, Ersa shall close the workbench, moving keyboard focus to the arrange button when it was inside the workbench. The menu item shall be marked while the workbench is open.
- **FLOW-READ-RAW-LOG-R02**: Ersa shall keep whether the workbench is open, and its width, across restarts, starting with it closed when neither has been kept; when it can't keep them it shall leave the workbench as the user set it and say nothing.
- **FLOW-READ-RAW-LOG-R03**: While the developer pane is open, Ersa shall list the active session's raw log oldest first, each line as one row with the time it crossed in the user's local time to the millisecond, its direction, and the start of its text, and shall give each row's direction in words to assistive technology.
- **FLOW-READ-RAW-LOG-R04**: When the active session changes while the pane is open, Ersa shall list the new active session's raw log from the first line Core holds, with no row selected, and shall show no line of the session it left.
- ~~**FLOW-READ-RAW-LOG-R05**~~: retired 2026-11-24, see FLOW-READ-RAW-LOG-R19.
- **FLOW-READ-RAW-LOG-R06**: While the workbench is closed, Ersa shall not ask for any session's raw log.
- **FLOW-READ-RAW-LOG-R07**: While the list shows its end, Ersa shall keep it at the end as lines arrive; once the user has scrolled away from the end, Ersa shall leave the rows in view where they are, say how many lines have arrived since, and offer Jump to latest, which returns to the end and to following.
- **FLOW-READ-RAW-LOG-R08**: Ersa shall keep in the page only the rows in or near view, however many lines the pane holds.
- **FLOW-READ-RAW-LOG-R09**: The pane shall hold at most the newest 50,000 lines of a session, and when an answer starts later than the line asked for, shall let go of the lines it holds and list from that answer's first line. Whenever lines before the first it lists are no longer held, by the pane or by Core, the list's first row shall say that earlier lines are no longer held.
- **FLOW-READ-RAW-LOG-R10**: When the user selects a row, Ersa shall show that line beneath the list as it crossed, indented as JSON when the whole line is valid JSON and unchanged otherwise, with Copy, which shall put the line as it crossed on the clipboard and say it is copied only once it is there. Closing the detail from inside it shall move keyboard focus to the list.
- **FLOW-READ-RAW-LOG-R11**: Ersa shall ask for every raw log line with `clip` 262144 (PROTO-CORE-API-R138), and for a line that came back cut shall say in its detail how large the whole line is and that its start is shown, shall show the start unchanged, and shall copy what is shown.
- **FLOW-READ-RAW-LOG-R12**: While the filter field holds text, Ersa shall list only the lines whose text contains it, compared without regard to case, shall say how many of the lines held are listed, and shall go on following as R07 says; emptying the field shall list every line again.
- **FLOW-READ-RAW-LOG-R13**: When the user chooses Open diagnostic log, Ersa shall open the diagnostic log's file with the system's app for it, and shall say so with the reason when it could not be opened; in a debug build attached to a labeled endpoint, the button shall be unavailable and give that reason.
- **FLOW-READ-RAW-LOG-R14**: With no active session the pane shall say to choose a session; with an active session whose raw log holds no line it shall say that lines appear while the session runs; until the first answer for a session arrives it shall say it is reading; and with a filter that no line contains it shall say so.
- **FLOW-READ-RAW-LOG-R15**: When a read of the raw log fails, the pane shall say so with the reason, keep the lines it holds, and go on asking as R19 says; when a later read succeeds the message shall go.
- **FLOW-READ-RAW-LOG-R16**: The user shall be able to resize the workbench by dragging its edge beside the transcript, and from the keyboard with the arrow keys while that edge has focus; its width shall never be more than half the width it shares with the transcript area, and never less than its least width while that half allows it.
- **FLOW-READ-RAW-LOG-R17**: The list shall take keyboard focus as one stop; with focus in it, Up and Down shall move the selection by a row, Home and End to the first and last row, and Escape shall clear the selection and close the detail, leaving focus in the list.
- **FLOW-READ-RAW-LOG-R18**: When Ersa connects to Core again while the pane is open, the pane shall let go of the lines it holds and read the active session's raw log from the first line Core holds.
- **FLOW-READ-RAW-LOG-R19**: While the pane is open with an active session, Ersa shall read that session's raw log from the line after the last it holds: at once when the pane opens, the active session changes or Ersa connects to Core again; again at once after a read that added a line to those the pane holds; and after any other read, a second after it asked for that read, or when that read ends if that is later.

## Out of scope

- Showing the diagnostic log or the app log inside the pane. The owner chose a button that opens the file (2026-11-22).
- Projected events, `backend.passthrough` among them: the pane shows lines, not events.
- Editing, replaying or sending lines, and saving the raw log to a file.
- A window of its own for the pane (owner, 2026-11-22).
- The other panes (Files, Diff, Terminal, Browser), and tabs or splits between panes: milestone M2.

## Open questions

- A keyboard shortcut that opens and closes the workbench, which waits on `LAY-SHELL`'s keyboard model.
- Hiding lines by kind, such as a streaming reply's partial messages, which flood the list while the agent writes. The filter shows the lines that contain a text; it can't hide those that do.
- Whether the pane's opener moves to the title bar's menu when Ersa has one, as Settings does.

## Changes

- 2026-11-24: from #568 inbox: R05 had the pane ask again at once only after an answer that held as many entries as one answer may, but an answer also ends at 8 MiB (PROTO-CORE-API-R139) with lines still waiting, about 32 lines cut at 256 KiB, and nothing in an answer says that more wait, so a run of long lines arrived about 32 a second. The pane now asks again at once after any read that added a line, and waits its second only after one that added none or failed (FLOW-READ-RAW-LOG-R19); only an answer with no line tells it that it has them all. R19 also says what R05 left to the reader: the first read goes at once, and the next waits for the one before. A call the owner can overrule: Core's answer stays as it is. Having Core say whether more lines wait would change the answer from a list to an object, for every client and the replay server, to save one read; the cost of asking instead is one more small read after each burst of lines, and reads that follow one another without a pause while lines arrive faster than a read takes, which is when the pane is behind. R19 added; R05 retired; R15 reworded to name R19, its meaning kept.
- 2026-11-22: created, for M1's week (ARCH-CORE, Milestones). The owner's decisions of 2026-11-22: the pane shows the raw lines and a button opens the diagnostic log's file; the pane lives in the main window's workbench. Calls made here that the owner can overrule: it opens from the arrange menu, above Settings, and has no keyboard shortcut yet; it reads by asking once a second, with no new notification; one row to a line at a fixed height, with the whole line beneath on selection; a filter by contained text; it holds 50,000 lines and asks for lines cut to 256 KiB; the workbench's open state and width are kept across restarts, for the window, not for each session. FLOW-READ-RAW-LOG-R01 to R18 added.
