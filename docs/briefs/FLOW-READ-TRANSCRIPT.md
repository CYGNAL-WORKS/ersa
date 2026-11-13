---
id: FLOW-READ-TRANSCRIPT
type: flow
title: Follow the agent's work
status: ready
references: [LAY-SHELL, PROTO-AGENT-EVENTS, FLOW-SEND-MESSAGE, FLOW-APPROVE-PERMISSION]
adrs: [0034]
---

# Follow the agent's work

## Outcome

The user reads what the agent said and did in a session, as formatted text with each tool call and each file change shown as a card, live while the agent works and the same when the session is opened later.

## Trigger

The user opens a session, or sends a message (`FLOW-SEND-MESSAGE`) and watches the turn.

## Steps

1. On `LAY-SHELL`'s transcript, the agent's words read as a document: Markdown rendered as formatted text, with code blocks the user can copy.
2. Each tool call shows as a tool card: one row with its state, the tool, what it acts on, and how long it took. The user opens a card to see the call's input and output, and closes it again.
3. A call that changes files shows as a diff card: the same row with the lines added and removed, and the change itself shown open from the start.
4. Calls a subagent makes sit inside the card of the call that started the subagent.
5. A link in the agent's words opens in the system's default handler: a web address in the browser, an email address in the mail app.

`docs/design/TRANSCRIPT.md` says how each of these looks.

## Variations and errors

- **A call that fails** says so on its card, and its output is the error. A diff card that fails keeps the change it proposed, so the user sees what was not applied.
- **A call waiting for the user's permission** says so on its card; the request itself is `FLOW-APPROVE-PERMISSION`'s.
- **Markdown Ersa does not show as itself**: raw HTML shows as its text, and an image shows as its alternative text linked to its address, never loaded (ADR 0034).
- **A link with any other scheme** than `http`, `https` or `mailto` shows as text and opens nothing.
- **A session opened from its record** shows the same cards with what the record keeps: a call's progress is live only, so a card read from the record shows its duration and no progress.

## Requirements

- **FLOW-READ-TRANSCRIPT-R01**: The transcript shall render the agent's text as Markdown with GitHub's extensions (tables, strikethrough, task lists, autolinks), as formatted text and never as HTML.
- **FLOW-READ-TRANSCRIPT-R02**: Raw HTML in the agent's text shall show as the text it is, and a Markdown image shall show as its alternative text linked to its address, without loading it.
- **FLOW-READ-TRANSCRIPT-R03**: When the user chooses a link in the agent's text whose scheme is `http`, `https` or `mailto`, Ersa shall open it in the system's default handler; a link with any other scheme shall open nothing.
- **FLOW-READ-TRANSCRIPT-R04**: Each code block shall offer Copy, which puts the block's text on the clipboard exactly, without its fences or language.
- **FLOW-READ-TRANSCRIPT-R05**: While the agent's text streams, a construct not yet closed (a code block, a list, a table) shall render as far as it has arrived, and the text before it shall not change as the rest arrives.
- **FLOW-READ-TRANSCRIPT-R06**: Each tool call shall show as a tool card: one row with its state (running, waiting for the user, done or failed), the tool's name, its target when the event carries one, and, once done, its duration, from `tool.result`'s `duration_ms` when the event carries one, else from its `tool.requested` to its `tool.result` by their events' `ts`, or, while it runs live, its latest progress or else its elapsed time.
- **FLOW-READ-TRANSCRIPT-R07**: A tool card shall be closed at first, and choosing its row shall open and close it; open, it shall show the call's input fields and its output, or its error for a failed call.
- **FLOW-READ-TRANSCRIPT-R08**: A target that is a path inside the session's workspace folder shall show relative to that folder, comparing the paths whatever separators each uses.
- **FLOW-READ-TRANSCRIPT-R09**: A tool call whose event carries `edits` shall show as a diff card, open at first, with one line diff per file, applying that file's edits in order, between its old and new text, and the counts of lines added and removed on its row; a file written whole shall show as all added lines; while the call waits for the user's permission, the diff card shall say so in its state and still show the change.
- **FLOW-READ-TRANSCRIPT-R10**: A tool call with a parent tool call shall show inside its parent's card, not as a block of the turn.
- **FLOW-READ-TRANSCRIPT-R11**: The agent's thinking shall show as one row saying the agent is thinking, or thought, and its summary when the event carries one, shown when the user opens the row.
- **FLOW-READ-TRANSCRIPT-R12**: Each card's row shall be reachable by keyboard and say to assistive technology the tool, its target, its state, and whether it is open.

## Out of scope

- Syntax colors in code blocks and diffs.
- Line numbers in a diff: a call carries the text it replaces, not where it sits.
- Copying a whole message, and the actions Claude Desktop offers beneath a turn.
- Showing a file's full diff against its base, which is the workbench's Diff pane (M2).

## Open questions

- Whether code blocks and diffs take syntax colors, and from which library (ADR 0015), once the owner has lived with them plain.
- Whether consecutive tool cards group into one row, as Claude Desktop groups a run of calls.
- Whether the permission card of a call that changes files shows the change itself, as Claude Desktop's prompt does, rather than its input fields beneath the diff card.

## Changes

- 2026-11-13: created ready for M1's transcript: formatted text, tool cards and diff cards, from the owner's Main board and Claude Desktop, under the owner's decision that `docs/design/TRANSCRIPT.md` is approved as a page (owner, 2026-11-13). Code stays without syntax colors and diffs without line numbers for now. FLOW-READ-TRANSCRIPT-R01 to R12 added.
