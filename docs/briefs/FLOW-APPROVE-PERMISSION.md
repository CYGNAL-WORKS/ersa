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

An agent asks to run a command, edit a file, or use a tool, or asks the user a question. The session's tile in the rail marks that it needs the user, and its chevron is the handle of the shelf. When the user chooses the chevron, the shelf pulls out of the tile over the edge of the transcript and holds the oldest pending request with its controls: what is being asked, and the session's context and cost so far. The user answers there, and the agent continues. The active session never changes: several sessions' requests are answered in turn from the rail, and opening the transcript is available for context but never required. `LAY-SHELL` says where the shelf sits; `docs/design/RAIL.md` (The shelf) says how it looks.

The shelf is the user's to open: a request arriving never opens it or moves focus, and the tile's chevron and its words are how Ersa says a session waits.

The shelf is for sessions other than the active one. The active session's requests are answered on its transcript's cards, so its tile's chevron opens no shelf, and making a session active closes its shelf. The shelf and a transcript card never show the same request at once.

The answers are the ones the request carries: what the backend accepts for it, as the agent offered them. For a permission request that is allowing, denying, and the request's own suggestions; for a question, its options, the user's own words where the question takes them, or declining to answer. Ersa invents no answer of its own.

What the shelf reads comes from the session's events (`PROTO-AGENT-EVENTS`, Permission and Question): Ersa follows the session's events while its shelf is open (`PROTO-CORE-API`, `session.subscribe`) and stops when it closes, and answers with `session.decide_permission` and `session.answer_question`. The tile's pending list says that a session waits and for how many requests; the events say what each request is, and their order is the requests' order. A request the shelf can't show, because its kind is unknown or its event is not held, is the oldest when Core has let go of the session's earliest events, and otherwise the newest (R29).

## Requirements

Opening and closing

- **FLOW-APPROVE-PERMISSION-R01**: When the user chooses the chevron of a tile whose session is not the active one and has a pending permission request or question, Ersa shall open that tile's shelf showing the session's oldest pending request, the one whose event came first, with a request it can't show placed as R29 says, and shall leave the active session unchanged.
- **FLOW-APPROVE-PERMISSION-R02**: Ersa shall open a shelf only when the user chooses its tile's chevron; a request arriving for any session shall not open a shelf or move keyboard focus.
- **FLOW-APPROVE-PERMISSION-R03**: When the user opens a shelf while another is open, Ersa shall close the other one, so that at most one shelf is open.
- **FLOW-APPROVE-PERMISSION-R04**: The open shelf shall overlay the transcript and shall not move, resize or scroll it.
- **FLOW-APPROVE-PERMISSION-R05**: When the shelf opens, Ersa shall move keyboard focus to the shelf itself, not to any of its answers, so that no single key answers a request the user has not read.
- **FLOW-APPROVE-PERMISSION-R06**: When the user presses Escape with focus in the shelf or on its chevron, or chooses the chevron again, Ersa shall close the shelf and put keyboard focus on the chevron.
- **FLOW-APPROVE-PERMISSION-R07**: When the user chooses anything outside the shelf and its tile, or keyboard focus leaves them both, Ersa shall close the shelf and leave focus where the user put it.
- **FLOW-APPROVE-PERMISSION-R08**: When the session has no pending request left, however its last one was answered, Ersa shall close the shelf; when keyboard focus was in the shelf, Ersa shall put it on the tile, since its chevron is gone.

More than one request

- **FLOW-APPROVE-PERMISSION-R09**: The shelf shall show the shown request's place as "n of m" whenever m is more than one, where n counts the requests the shelf has shown since it opened that are no longer pending, plus one, and m is n plus the requests still pending after the shown one; a request that arrives while the shelf is open adds to m, and one answered elsewhere before the shelf showed it leaves m without adding to n.
- **FLOW-APPROVE-PERMISSION-R10**: When the shown request is answered and others are pending, the shelf shall stay open and show the next oldest, with keyboard focus on the shelf itself.

A permission request

- **FLOW-APPROVE-PERMISSION-R11**: For a permission request, the shelf shall show "Permission request"; what is asked, being the request's description, else the tool's display name, else its name; the tool's input as the transcript's permission card shows it; the blocked path and the reason when the request gives them; and the session's context ring and cost as its tile shows them.
- **FLOW-APPROVE-PERMISSION-R12**: For a permission request, the shelf shall offer the answers the request carries: Deny, Allow, and one answer per suggestion in the request, labeled with the suggestion's label, and no suggestion when the request's flags suppress always allowing.
- **FLOW-APPROVE-PERMISSION-R13**: When the user chooses Allow, Deny, or a suggestion, Ersa shall send `session.decide_permission` with behavior `allow`, `deny`, or `allow` with that suggestion's index in `apply_suggestions`, respectively, and no message or updated input.
- **FLOW-APPROVE-PERMISSION-R14**: Allow shall be the shelf's primary answer, except when the request's flags say it defaults to no, when Deny shall be.

A question

- **FLOW-APPROVE-PERMISSION-R15**: For a question, the shelf shall show "Question", then each of the request's questions with its header when it has one, its text and, beneath it, its options with their descriptions as choices of one, or of any number when the question allows several; and a text field for the user's own words when the question has no options or takes text beside them.
- **FLOW-APPROVE-PERMISSION-R16**: The shelf shall offer Answer for a question, available once every question in the request has an answer; when the user chooses it, Ersa shall send `session.answer_question` with one answer per question: the chosen option's id or the chosen options' ids, or the user's own words with `own_words` set (`PROTO-AGENT-EVENTS`, `question.answered`).
- **FLOW-APPROVE-PERMISSION-R17**: When the active session has a pending question, the transcript's question card shall offer the same answers as the shelf would.
- **FLOW-APPROVE-PERMISSION-R26**: The chevron on the active session's tile shall open no shelf, and when the user makes the session whose shelf is open the active one, Ersa shall close the shelf.
- **FLOW-APPROVE-PERMISSION-R27**: The transcript's permission card shall offer the same answers as the shelf would for that request, with the same primary answer.
- **FLOW-APPROVE-PERMISSION-R28**: For a question, the shelf and the transcript's question card shall also offer Decline, available at once; when the user chooses it, Ersa shall send `session.answer_question` with `declined` and no answers.

Sending an answer

- **FLOW-APPROVE-PERMISSION-R18**: When the user answers from the shelf, Ersa shall send that one answer and make the request's answers unavailable while it is on its way, in the shelf and on the transcript's card if the user makes the session active meanwhile.
- **FLOW-APPROVE-PERMISSION-R19**: When Core refuses an answer, the shelf shall say why, in the user's terms, and offer the request's answers again.
- **FLOW-APPROVE-PERMISSION-R20**: While Ersa can't reach Core, the shelf's answers shall be unavailable and the shelf shall say that Ersa is reconnecting.
- **FLOW-APPROVE-PERMISSION-R21**: While Ersa is reading the session's events for the shelf, the shelf shall say it is loading; when reading them fails, it shall say why and offer Retry.
- **FLOW-APPROVE-PERMISSION-R25**: When the shown request is of a kind Ersa doesn't know, or its event is no longer among the session's events Core holds, the shelf shall say that the session is waiting on a request Ersa can't show here, and offer no answers for it.
- **FLOW-APPROVE-PERMISSION-R29**: When ordering a session's pending requests, the shelf shall count a request it can't show (R25) as the oldest when the session's events it holds start after events Core let go of, and otherwise as newer than every request it can show.

Assistive technology

- **FLOW-APPROVE-PERMISSION-R22**: The chevron of a session that is not the active one shall be a button named "Request from" and the session's title that states whether its shelf is open.
- **FLOW-APPROVE-PERMISSION-R23**: The shelf shall be a dialog that leaves the rest of the window usable, named "Request from" and the session's title, read in the order it is shown: what kind, what is asked, its details, the session's readouts, then the answers.
- **FLOW-APPROVE-PERMISSION-R24**: When the shelf shows the next request after an answer, Ersa shall tell assistive technology politely what is now shown and its place, "n of m".

## Open questions

- How much of each backend's native permission vocabulary is surfaced versus normalized: a reason with Deny (`message`) and editing the input before allowing (`updated_input`) are not offered yet.
- Keyboard-only path: a shortcut that opens the oldest pending shelf across sessions, and moves to the next session's once one is done.
- Whether a request the shelf can't show should hold the shelf when requests behind it can be shown. Today the requests behind it are answered on the transcript's cards.
- Whether Core should carry each pending request's details on the tile, so that opening a shelf does not read the session's whole event history first.

## Changes

- 2026-11-06: from #222 inbox: R09's n counts only requests the shelf showed, so one answered elsewhere before it was shown leaves m (R09 reworded).
- 2026-11-06: from #215 inbox: where a request the shelf can't show falls in the order (R29 added; R01 reworded to place such a request as R29 says).
- 2026-11-05: a question can be declined (R28 added), as Claude Code accepts (PROTO-CLAUDE-CODE-STREAM, Questions).
- 2026-11-05: owner review: the active session has no shelf (R26 added; R27 gives the transcript's permission card the request's own answers; R01 narrowed to other sessions; R18 no longer joins the shelf and the transcript's card); the shelf shows no workspace (R11, R15); the answers are the ones the request carries (R12 reworded); a question shows its header and option descriptions and takes the user's own words where it allows them (R15, R16). Claude Code's questions are projected and answered (PROTO-CLAUDE-CODE-STREAM, Questions).
- 2026-11-05: specified from the owner's decisions and the Rail mockup's shelf: opened only from the chevron, one at a time, over the transcript's edge, oldest request first with its place, focus in and back, what a permission request and a question show and send, one answer between the shelf and the transcript's card, and what assistive technology hears. Status ready; FLOW-APPROVE-PERMISSION-R01 to R25 added. Closing on the last answer puts focus on the tile rather than on the chevron the owner named, since the chevron goes once nothing is pending (R08). Closes the open questions on an "approve all" option (the request's own suggestions, never offered when it suppresses them) and on large details (they scroll, as the transcript's card's do).
- 2026-11-02: US spelling (ADR 0020, docs/design/CONTENT.md). Meaning unchanged; no requirement IDs changed.
- 2026-11-01: from #14 inbox: M0 answers in the transcript with Allow or Deny; which answer options the shelf offers is open.
- 2026-10-31: reframed around the shelf; the transcript is a drill-down, not the place to answer.
- 2026-10-31: created as draft.
