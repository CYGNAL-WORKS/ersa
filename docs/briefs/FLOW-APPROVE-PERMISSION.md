---
id: FLOW-APPROVE-PERMISSION
type: flow
title: Answer a session from the shelf
status: ready
references: [LAY-SHELL, PROTO-CORE-API, PROTO-AGENT-EVENTS, PROTO-HARNESS-TOOLS]
adrs: [0012]
---

# Answer a session from the shelf

## Intent

An agent asks to run a command, edit a file, or use a tool, or asks the user a question. The session's tile in the rail marks that it needs the user, and its chevron is the handle of the shelf. When the user chooses the chevron, the shelf pulls out of the tile over the edge of the transcript and holds the oldest pending request with its controls: what is being asked, where, and the session's context and cost so far. The user answers there, and the agent continues. The active session never changes: several sessions' requests are answered in turn from the rail, and opening the transcript is available for context but never required. `LAY-SHELL` says where the shelf sits; `docs/design/RAIL.md` (The shelf) says how it looks.

The shelf is the user's to open. A request arriving never opens it, never moves focus, and never covers the transcript on its own; the tile's chevron and its words are how Ersa says a session waits. One shelf is open at a time, and it closes on Escape, when the user chooses something else, or once its session has nothing left pending.

The request card the transcript shows for the active session and the shelf are two views of one request. Either can answer it; the first answer sent is the one that counts, and both show it being sent and then the decision once Core reports it.

What the shelf reads comes from the session's events (`PROTO-AGENT-EVENTS`, Permission and Question): Ersa follows the session's events while its shelf is open (`PROTO-CORE-API`, `session.subscribe`), as it does for the active session, and answers with `session.decide_permission` and `session.answer_question`. The tile's pending list says that a session waits and for how many requests; the events say what each request is.

## Requirements

Opening and closing

- **FLOW-APPROVE-PERMISSION-R01**: When the user chooses the chevron of a tile whose session has a pending permission request or question, Ersa shall open that tile's shelf showing the session's oldest pending request, the one whose event came first, and shall leave the active session unchanged.
- **FLOW-APPROVE-PERMISSION-R02**: Ersa shall open a shelf only when the user chooses its tile's chevron; a request arriving for any session shall not open a shelf or move keyboard focus.
- **FLOW-APPROVE-PERMISSION-R03**: When the user opens a shelf while another is open, Ersa shall close the other one, so that at most one shelf is open.
- **FLOW-APPROVE-PERMISSION-R04**: The open shelf shall overlay the transcript and shall not move, resize or scroll it.
- **FLOW-APPROVE-PERMISSION-R05**: When the shelf opens, Ersa shall move keyboard focus to the shelf itself, not to any of its answers, so that no single key answers a request the user has not read.
- **FLOW-APPROVE-PERMISSION-R06**: When the user presses Escape with focus in the shelf or on its chevron, or chooses the chevron again, Ersa shall close the shelf and put keyboard focus on the chevron.
- **FLOW-APPROVE-PERMISSION-R07**: When the user chooses anything outside the shelf and its tile, or keyboard focus leaves them both, Ersa shall close the shelf and leave focus where the user put it.
- **FLOW-APPROVE-PERMISSION-R08**: When the session has no pending request left, however its last one was answered, Ersa shall close the shelf; when keyboard focus was in the shelf, Ersa shall put it on the tile, since its chevron is gone.

More than one request

- **FLOW-APPROVE-PERMISSION-R09**: While more than one request is pending, the shelf shall show the shown request's place as "n of m", where n counts the requests answered since the shelf opened plus one, and m is n plus the requests still pending after the shown one; a request that arrives while the shelf is open adds to m.
- **FLOW-APPROVE-PERMISSION-R10**: When the shown request is answered and others are pending, the shelf shall stay open and show the next oldest, with keyboard focus on the shelf itself.

A permission request

- **FLOW-APPROVE-PERMISSION-R11**: For a permission request, the shelf shall show "Permission request" and the name of the session's workspace folder; what is asked, being the request's description, else the tool's display name, else its name; the tool's input as the transcript's permission card shows it; the blocked path and the reason when the request gives them; and the session's context ring and cost as its tile shows them.
- **FLOW-APPROVE-PERMISSION-R12**: For a permission request, the shelf shall offer Deny, Allow, and one answer per suggestion the request carries, labeled with the suggestion's label; it shall offer no suggestion when the request's flags suppress always allowing.
- **FLOW-APPROVE-PERMISSION-R13**: When the user chooses Allow, Deny, or a suggestion, Ersa shall send `session.decide_permission` with behavior `allow`, `deny`, or `allow` with that suggestion's index in `apply_suggestions`, respectively, and no message or updated input.
- **FLOW-APPROVE-PERMISSION-R14**: Allow shall be the shelf's primary answer, except when the request's flags say it defaults to no, when Deny shall be.

A question

- **FLOW-APPROVE-PERMISSION-R15**: For a question, the shelf shall show "Question" and the name of the session's workspace folder, then each of the request's questions with its text and, beneath it, its options as choices of one, or of any number when the question allows several, or a text field when it has no options.
- **FLOW-APPROVE-PERMISSION-R16**: The shelf shall offer Answer for a question, available once every question in the request has an answer; when the user chooses it, Ersa shall send `session.answer_question` with one answer per question, its value the text, the chosen option's id, or the chosen options' ids (`PROTO-AGENT-EVENTS`, `question.answered`).
- **FLOW-APPROVE-PERMISSION-R17**: When the active session has a pending question, the transcript's question card shall offer the same answers as the shelf.

Sending an answer

- **FLOW-APPROVE-PERMISSION-R18**: When a request is answered from the shelf or from the transcript's card, Ersa shall send that one answer, make the request's answers unavailable in both while it is on its way, and show the decision in both once Core reports it.
- **FLOW-APPROVE-PERMISSION-R19**: When Core refuses an answer, the shelf shall say why, in the user's terms, and offer the request's answers again.
- **FLOW-APPROVE-PERMISSION-R20**: While Ersa can't reach Core, the shelf's answers shall be unavailable and the shelf shall say that Ersa is reconnecting.
- **FLOW-APPROVE-PERMISSION-R21**: While Ersa is reading the session's events for the shelf, the shelf shall say it is loading; when reading them fails, it shall say why and offer Retry.

Assistive technology

- **FLOW-APPROVE-PERMISSION-R22**: The chevron shall be a button that states whether its shelf is open, named for its action and its session ("Open the request from" or "Close the request from", then the session's title).
- **FLOW-APPROVE-PERMISSION-R23**: The shelf shall be a dialog that leaves the rest of the window usable, named "Request from" and the session's title, read in the order it is shown: what kind, where, what is asked, its details, the session's readouts, then the answers.
- **FLOW-APPROVE-PERMISSION-R24**: When the shelf shows the next request after an answer, Ersa shall tell assistive technology politely what is now shown and its place, "n of m".

## Open questions

- How much of each backend's native permission vocabulary is surfaced versus normalized: a reason with Deny (`message`) and editing the input before allowing (`updated_input`) are not offered yet.
- Keyboard-only path: a shortcut that opens the oldest pending shelf across sessions, and moves to the next session's once one is done.
- Whether Core should carry each pending request's details on the tile, so that opening a shelf does not read the session's whole event history first.
- How a question that Claude Code asks reaches Ersa: until its adapter projects one to `question.asked` (`PROTO-AGENT-EVENTS`, open questions), it arrives as a permission request for the asking tool.

## Changes

- 2026-11-05: specified from the owner's decisions and the Rail mockup's shelf: opened only from the chevron, one at a time, over the transcript's edge, oldest request first with its place, focus in and back, what a permission request and a question show and send, one answer between the shelf and the transcript's card, and what assistive technology hears. Status ready; FLOW-APPROVE-PERMISSION-R01 to R24 added. Closes the open questions on an "approve all" option (the request's own suggestions, never offered when it suppresses them) and on large details (they scroll, as the transcript's card's do).
- 2026-11-02: US spelling (ADR 0020, docs/design/CONTENT.md). Meaning unchanged; no requirement IDs changed.
- 2026-11-01: from #14 inbox: M0 answers in the transcript with Allow or Deny; which answer options the shelf offers is open.
- 2026-10-31: reframed around the shelf; the transcript is a drill-down, not the place to answer.
- 2026-10-31: created as draft.
