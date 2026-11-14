---
id: PROTO-AGENT-EVENTS
type: protocol
title: Agent event protocol
status: ready
references: []
adrs: [0004, 0011, 0014, 0017]
---

# Agent event protocol

Ersa's own vocabulary for what happens in a session, independent of any backend. Every event is projected from the backend's session record or its live stream (ADR 0017), or emitted by Core for the session's lifecycle: adapters provide the projection function, Core applies it each time a session is opened and stores nothing projected, and the Interface renders events. Commands in the other direction (send a message, decide a permission, interrupt) are not events; they are `PROTO-CORE-API` requests.

The machine-readable form is `protocol/schema/agent-events.schema.json` at the repository root, maintained by the implementing ticket with conformance tests against the fixtures. This brief defines the vocabulary the schema must express.

## Parties

Core produces events (by projecting the session record and the live stream, and from its own knowledge of the backend process) and streams them to subscribed Interfaces as `PROTO-CORE-API` notifications. Adapters produce the projection but never talk to the Interface.

## Envelope

Every event is an object with:

- `v`: event schema version, an integer. Bumped when any event's meaning or shape changes. Nothing projected is stored, so a fix to an adapter's projection applies the next time a session is opened.
- `session_id`: Ersa's session identifier (not the backend's).
- `seq`: a monotonically increasing integer within the session's view (see Opening a session), assigned by Core, gap-free: the session record's events first, then the live stream's. A new view numbers from the start again. Used for resubscribing (`from_seq`) and ordering.
- `ts`: ISO 8601 time of the input the event came from: the time the session record gives the entry, or the time Core received the live line. Never the clock at projection time, so projecting the same input again yields the same value.
- `source`: `{ backend, backend_version, input, message_id (optional) }`: which backend and version produced the event's input; `input` is `record` (the session record), `live` (the live stream) or `core` (Core's own lifecycle facts); `message_id` is the backend's id for the message the event came from, on which record and live input are merged.
- `event`: the typed payload; `event.type` discriminates.

## Event types

Names are `family.verb`. Fields listed are required unless marked optional.

### Session

- `session.started`: `backend`, `backend_version`, `model`, `cwd`, `capabilities` (string list), `backend_session_id` (optional). Projected from the backend's first announcement (Claude Code `system/init`).
- `session.state`: `state` one of `starting`, `running`, `waiting_user`, `idle`, `disconnected`, `ended`, `error`; `reason` (optional short string). Emitted by Core from what it knows of the backend process, and projected from live lines (a pending permission request means `waiting_user`). A fork's backend, or a resumed session's, reports nothing until its next message, so each goes from `starting` to `idle` when its backend process answers Core's first request (for Claude Code, `initialize`).
- `session.ended`: `reason` one of `user_stopped`, `backend_exited`, `error`; `exit_code` (optional). Emitted by Core.
- `session.notice`: `level` one of `info`, `warning`; `code` one of `backend_version_untested`, `session_open_elsewhere`, `session_open_check_failed`; `message`; `pid` (optional: with `session_open_elsewhere`, the process that has the session open). Something the user should know that is not an error, such as a backend newer than the newest version Ersa was tested against. Core emits `session_open_elsewhere` at `warning` when the adapter's live-session check finds another process with the session open, and `session_open_check_failed` at `info` when the check could not tell. It asks before it starts the backend to resume a session, and before it appends a title to the record of a session it is not running. The first is a known risk that two processes write one record; the second is a doubt, not a known risk.

### Turn

- `turn.started`: `turn` (1-based index), `trigger` one of `user`, `resume`, `system`.
- `turn.completed`: `turn`, `outcome` one of `success`, `error`, `interrupted`, `max_turns`; `duration_ms`, `is_error`, `final_text` (optional).

### User

- `user.message`: `text`, `origin` one of `human`, `peer_session`, `system`; `peer` (optional: `{ session_id, name }`); `attachments` (optional: in the order sent, each `{ kind: image, media_type, data }` with base64 `data`, or `{ kind: file | folder, path }` with the path as the agent was given it, relative to the workspace; PROTO-CORE-API, Turns and attachments). Projected from a user message in the session record, or emitted by Core when it sends one; a message written during a turn is emitted when its own turn starts (PROTO-CLAUDE-CODE-STREAM-R43).

### Assistant

- `assistant.text`: `block_id`, `text`, `final` (boolean). With partial streaming, several events share a `block_id` with growing `text` and `final: false`, then one with `final: true`. Without partial streaming, one event with `final: true`.
- `assistant.thinking`: `block_id`, `summary` (optional text), `final`. Present even when the backend withholds content, so the Interface can show that thinking happened.

### Tool

- `tool.requested`: `tool_use_id`, `name`, `display_name` (optional), `input` (object), `parent_tool_use_id` (optional, for subagent trees), `target` (optional: the one thing the call acts on, as the user would name it: a path, a command, a search, an address, a subagent's task), `target_kind` (present with `target`: `path`, `command`, `search`, `address` or `task`, so the Interface knows a path without guessing from its spelling), `edits` (optional: for a call that changes files, one `{ path, old_text (optional), new_text, every_occurrence (optional) }` per change in the order the call makes them, where `old_text` is the text replaced and is absent when the call writes the whole file, and `every_occurrence` is true when the change replaces that text everywhere it occurs in the file, not once). The adapter fills `target`, `target_kind` and `edits` from its backend's own tools, so the Interface never reads a backend's tool input to draw a card.
- `tool.result`: `tool_use_id`, `content` (string or structured), `is_error`, `duration_ms` (optional).
- `tool.progress`: `tool_use_id`, `detail` (optional string). Projected from backend progress and task-summary lines.

### Permission

- `permission.requested`: `request_id`, `tool_use_id`, `tool_name`, `display_name` (optional), `description` (optional), `input` (object), `blocked_path` (optional), `suggestions` (array of `{ kind, label, payload }` where `kind` is `allow_rule`, `allow_directory`, or `set_mode` and `payload` is backend-specific and opaque to the Interface), `flags` (object of booleans: `default_to_no`, `suppress_always_allow`, `requires_user_interaction`, `classifier_approvable`), `reason` (optional: `{ kind, text }`).
- `permission.decided`: `request_id`, `behavior` one of `allow`, `deny`; `decided_by` one of `user`, `rule`, `backend`, `timeout`; `message` (optional); `applied_suggestions` (optional array of suggestion indices).
- `permission.denied_unanswered`: `tool_use_id`, `tool_name`, `message`. The backend denied because nobody could answer (observed when the control channel is not set up). Should not occur in a correctly configured session; its presence is a defect signal.

### Question

- `question.asked`: `request_id`, `questions` (array of `{ id, text, header (optional short label), options (optional array of { id, label, description (optional) }), multi_select (boolean), takes_text (boolean: the user may answer in their own words beside the options) }`). Each question's `id` is unique within the request.
- `question.answered`: `request_id`, `answers` (array of `{ id, value, own_words (boolean) }`, at most one per question, a question the backend's answer did not cover having none, where `value` is the user's own words when `own_words` is true, and otherwise a single option id, or an array of option ids for multi-select; empty when the user declined or the backend withdrew the question), `declined` (boolean: the user chose not to answer), `answered_by` one of `user`, `timeout`, `backend` (the backend withdrew the question, after an interrupt say).

### Usage

- `usage.updated`: `context_tokens` (optional), `context_window` (optional), `cost_usd_total` (optional), `turns` (optional: the number of Ersa turns started in the session, not the backend's API round trips), `turn_output_tokens` (optional: the output tokens the backend has generated so far in the current turn, across all of its API calls in that turn; projected from live input only, so a turn read from the session record has none; it belongs to one turn, so the Interface drops the last known value at `turn.started`), `rate_limit` (optional: `{ status, resets_at (optional), detail (optional) }`). Any field absent means "no new information"; the Interface keeps the last known value per field. Projected from result usage, rate-limit events, and thinking-token estimates.

### Error

- `error`: `code` one of `authentication_failed`, `backend_exited`, `protocol`, `backend_error`, `transport`; `message`; `recoverable` (boolean); `action` (optional: `log_in`, `restart`, `reconnect`).

### Passthrough

- `backend.passthrough`: `backend`, `kind` (the backend's own type and subtype joined with `/`), `payload` (the raw object, or the verbatim line as a string when it is not valid JSON). Every raw log line that maps to no other event maps to exactly one of these. The Interface shows them only in the developer pane.

## Projection rules

- A session's events have two inputs. The session record is its history; the adapter reads the entries that carry messages and titles and ignores the rest. The live stream is what a running backend emits; details only it carries (text as it arrives, permission prompts, turn ends, cost, rate limits) are shown and not stored, so the events they yield arise only from live input.
- A record entry carrying a user's message projects to `user.message` with origin `human` unless the backend wrote it, in which case its origin is `system` and it is never a session's title. A backend's protocol brief may name entries the backend writes that project only to `backend.passthrough`, such as Claude Code's `[Image: source: …]` companion (PROTO-CLAUDE-CODE-STREAM-R34). For Claude Code the CLI wrote it when the entry has `isMeta` or `isCompactSummary`, an `origin.kind` other than `human`, or text in the CLI's own markup: slash-command and local-command tags, bash input, task notifications, and the interrupted-request marker.
- When a session is opened, Core projects its record, then appends events projected from the live stream. A message present in both, identified by the backend's message id, yields its events once. A resumed backend repeats none of the record on its stream (Claude Code: fixtures 2.1.316/28-resume-original and 2.1.316/29-resume-while-open), and Core does not read the record again while a view is live, so a view kept across a resume is the events it held, then the live ones.
- Core's lifecycle facts (process started, start failed, a fork's or a resumed session's process ready, process exited with its code and whether the user stopped it, Core restarted while the session was live, which ends it with reason `error`) reach the session only as the `session.state` and `session.ended` events Core emits; Core also writes them to the app log. They are never part of the session record.
- The raw log holds the live lines in both directions, each tagged: `backend` (what the backend emitted) and `host` (what Ersa wrote to it). Host lines project too (a user message to `user.message`, a permission answer to `permission.decided`, the answer to a question to `question.answered`).
- Every raw log line yields at least one event (possibly only `backend.passthrough`); no line is dropped.
- A raw line may yield several events (a Claude Code `result` yields `turn.completed` and `usage.updated`).
- Events never contain backend-specific field names outside `backend.passthrough.payload` and `permission.requested.suggestions[].payload`. Tool data is not backend protocol: `tool.requested.input`, `permission.requested.input`, and structured `tool.result.content` carry the tool's own arguments and output as the tool defines them.
- Projection is a pure function of its input and the adapter version; running it again over the same session record and raw log yields the same events with the same `seq`.

## Opening a session

Opening a session is projecting it into a view: the events Core holds for the session's subscribers, with its own identity (`view` in `PROTO-CORE-API`'s `session.subscribe`) and its own sequence numbers.

- A session Core runs is projected when its backend starts, unless a view of it has a subscriber, and is not projected again while it is live. A view that has a subscriber when its session is resumed stays: the resumed run's events are appended to it, numbered on from its last `seq`. A kept view is not rebuilt, so entries another process wrote to the record since it was built show only in a view built later.
- A session Core does not run is projected when it is first subscribed. It is projected afresh, as a new view, on a subscribe from `seq` 0 while it has no other subscribers, so turns taken elsewhere show, and when it is first opened after Core restarts.
- A view that has no subscribers and whose session is not running may be dropped; the next subscribe builds a new one.
- A `session.notice` Core emits is in no record, so it lives only in the view open when Core emits it, where it takes a `seq`. A view built later does not contain it; with no view open, Core traces it and no client sees it. This is the contract, not a gap: a notice tells what was true when it was emitted, such as another process having the session open, and replayed later it could tell the user something no longer true. Nothing projected is stored (ADR 0017).

## Sequencing

Within one run of a session's backend: `session.started` first; `turn.started` before any `assistant.*`, `tool.*`, or `permission.*` of that turn; `permission.decided` after its `permission.requested`; `tool.result` after its `tool.requested`; `turn.completed` after everything in the turn; `session.ended` last. A view kept across a resume holds one run after another, so a resumed run's `session.state`, `session.started` and `session.ended` follow the earlier run's `session.ended`. `usage.updated`, `session.state`, `error`, and `backend.passthrough` may appear anywhere. These rules govern live input; events projected from a session record carry only the event types its entries yield.

## Errors and recovery

A projection failure on one raw line or record entry yields an `error` event with `code: protocol`, `recoverable: true`, plus a `backend.passthrough` of the offending line, and projection continues. Projection never aborts a session.

## Versioning

`v` on every event. The schema file carries the same version. The lifecycle events Core emits itself are part of the event schema's meaning, so any change to them bumps `v`. The Interface refuses to render a `v` it does not know and says that the window is older than Core and needs updating, since Core projects only at its own `v`.

## Conformance

The implementing crate projects every fixture in `docs/briefs/protocol/fixtures/claude-code/*/`, live stream and session record alike, and the result validates against the schema; a golden file per fixture records the expected event list and is reviewed when it changes.

## Requirements

- **PROTO-AGENT-EVENTS-R01**: Every event shall carry `v`, `session_id`, `seq`, `ts`, `source`, and `event`, and `seq` shall be gap-free and increasing within a session.
- **PROTO-AGENT-EVENTS-R02**: Every raw log line shall project to at least one event, with `backend.passthrough` as the fallback.
- **PROTO-AGENT-EVENTS-R03**: Projecting the same raw log twice with the same adapter version shall yield identical events.
- **PROTO-AGENT-EVENTS-R04**: A Claude Code `can_use_tool` control request shall project to one `permission.requested` carrying tool name, input, description, blocked path, suggestions, and flags.
- **PROTO-AGENT-EVENTS-R05**: A Claude Code `result` line shall project to one `turn.completed` and one `usage.updated` carrying total cost and token usage.
- ~~**PROTO-AGENT-EVENTS-R06**~~: retired 2026-11-13 (a session Claude Code started in its minimal mode is not signed out), replaced by R22.
- **PROTO-AGENT-EVENTS-R07**: With partial streaming enabled, `assistant.text` events for one block shall share a `block_id` and end with exactly one `final: true`.
- **PROTO-AGENT-EVENTS-R08**: No event outside `backend.passthrough.payload` and `permission.requested.suggestions[].payload` shall contain a backend-specific field name; tool-defined data in `tool.requested.input`, `permission.requested.input`, and `tool.result.content` is not backend protocol.
- **PROTO-AGENT-EVENTS-R09**: Every projected event shall validate against `protocol/schema/agent-events.schema.json`.
- **PROTO-AGENT-EVENTS-R10**: A projection failure on a line shall yield a recoverable `error` event and continue with the next line.
- **PROTO-AGENT-EVENTS-R11**: Projecting the same session record twice with the same adapter version shall yield identical events.
- **PROTO-AGENT-EVENTS-R12**: When a session is opened while its backend is live, a message present in both the session record and the live stream, identified by the backend's message id, shall yield its events once.
- **PROTO-AGENT-EVENTS-R13**: Core shall emit `session.state` and `session.ended` for the backend process's lifecycle from its own knowledge of the process, never from the session record, and shall write the same facts to the app log.
- **PROTO-AGENT-EVENTS-R14**: Before starting the backend to resume a session, Core shall emit a `session.notice` with code `session_open_elsewhere` and the process's `pid` when the adapter reports the session open in another process, and with code `session_open_check_failed` when the adapter cannot tell.
- **PROTO-AGENT-EVENTS-R15**: Before appending a title to the record of a session it is not running, Core shall emit a `session.notice` with code `session_open_elsewhere` and the process's `pid` when the adapter reports the session open in another process, and with code `session_open_check_failed` when the adapter cannot tell, and shall rename the session in every case.
- **PROTO-AGENT-EVENTS-R16**: Core shall emit `session_open_elsewhere` notices at level `warning` and `session_open_check_failed` notices at level `info`.
- **PROTO-AGENT-EVENTS-R17**: While a turn runs, each Claude Code live line that reports output usage (a partial `message_delta`, or an `assistant` line) shall project to a `usage.updated` whose `turn_output_tokens` is the sum of the output tokens of every API message in the turn so far, counting each message id once at the highest count reported for it, so the sum never decreases within the turn. A synthetic `assistant` message (model `<synthetic>`, made without an API call) adds nothing to the count.
- **PROTO-AGENT-EVENTS-R18**: A Claude Code `can_use_tool` control request for its question tool shall project to one `question.asked`, and no `permission.requested`, carrying each question's text, header, options with their labels and descriptions, and multi-select, with `takes_text` true (PROTO-CLAUDE-CODE-STREAM, Questions).
- **PROTO-AGENT-EVENTS-R19**: The host's answer to a Claude Code question shall project to one `question.answered` with `answered_by` `user`, `declined` when it was a deny, and no `permission.decided`, matching each answer to its question by text and to options by label as PROTO-CLAUDE-CODE-STREAM (Questions) says; the CLI withdrawing a pending question, or the turn ending with it unanswered, shall project to `question.answered` with `answered_by` `backend` and no answers.
- **PROTO-AGENT-EVENTS-R20**: A `tool.requested` for a call that replaces text in a file or writes a whole file shall carry `edits`, one entry per change with the file's path, the text replaced (absent for a whole file written) and the new text; any other call shall carry none.
- **PROTO-AGENT-EVENTS-R21**: A `tool.requested` for a call the adapter knows to act on one path, command, search or address shall carry it as `target`; any other call shall carry none.
- **PROTO-AGENT-EVENTS-R22**: A Claude Code `authentication_failed` synthetic message shall project to one `error`, and no `assistant.text`: with `code: authentication_failed` and `action: log_in`, except in a live session the CLI started in its minimal mode, which PROTO-CLAUDE-CODE-STREAM-R53 governs; read from a session record, it always projects this way.
- **PROTO-AGENT-EVENTS-R23**: An entry of `edits` whose change replaces its text everywhere it occurs in the file shall carry `every_occurrence` true; any other entry shall carry no `every_occurrence`.
- **PROTO-AGENT-EVENTS-R24**: When Core resumes a session whose view has a subscriber, it shall keep that view and append the resumed run's events to it, numbered on from the view's last `seq`, without projecting the session record again.
- **PROTO-AGENT-EVENTS-R25**: A resumed session's `session.state` shall go from `starting` to `idle` when its backend process answers Core's first request.
- **PROTO-AGENT-EVENTS-R26**: A `tool.requested` that carries `target` shall carry `target_kind`, one of `path`, `command`, `search`, `address` or `task`, saying which the target is; one without `target` shall carry none.

## Out of scope

- Commands from the Interface (see `PROTO-CORE-API`).
- Rendering decisions; this protocol says what happened, not how it looks.

## Open questions

- Whether a view kept across a resume is rebuilt when the record gained entries elsewhere since the view was built, so that turns taken in the backend's own tools show at once.
- Whether subagent trees get their own `session_id` or stay nested under `parent_tool_use_id`.
- How history shows a permission decision read from the session record, which keeps no permission prompt or request id, and which turn boundaries history shows without the turn ends the record omits. Claude Code records the decision on the tool result with `decision`, `source` (such as `config` or `user_temporary`), `reasonType`, and `grantScope`; `permission.decided` needs a request id, so no event carries it yet.
- How the transcript shows a question once. Claude Code's question tool also projects, live and from the record, to `tool.requested` and `tool.result` (R18 rules out only `permission.requested`), and the record keeps no `question.asked`, so history has only the tool events; the interface work that shows a question in the transcript settles whether those tool events show beside the question card, live and in history.
- Whether a Claude Code record entry with a `peer` origin maps to `user.message` origin `peer_session`, and how its `peer` is filled.

## Changes

- 2026-11-14: from #360 inbox: `tool.requested` gains `target_kind`, since a target's spelling does not say whether it is a path: a command can begin with a path, and a search pattern can look like one. A shape change, so the ticket that builds it bumps `v`. PROTO-AGENT-EVENTS-R26 added.
- 2026-11-13: from #353 inbox: a view with a subscriber when its session is resumed stays and takes the resumed run's events, so Sequencing's order holds within one run of the backend; a resumed session goes to `idle` when its backend is ready, as a fork does, a new lifecycle fact; the open question on merging record and live input is closed by the resume fixtures, with no exception to R02, and one on turns taken elsewhere before a resume replaces it. `v` is not bumped: no event changes shape or value, as when the fork's ready fact was added (2026-11-08). PROTO-AGENT-EVENTS-R24 and R25 added.
- 2026-11-13: from #350 inbox: R22 points at PROTO-CLAUDE-CODE-STREAM-R53, which replaced R51. Reworded, its ID unchanged.
- 2026-11-13: from #351 inbox: an entry of `edits` gains `every_occurrence`, so a diff card can say a change is made in more places than the one it shows. Part of the same shape change as `edits`, so `v` is not bumped again. PROTO-AGENT-EVENTS-R23 added.
- 2026-11-13: from #148: PROTO-AGENT-EVENTS-R06 retired for R22, which excepts a session Claude Code started in its minimal mode (PROTO-CLAUDE-CODE-STREAM-R51).
- 2026-11-13: for FLOW-READ-TRANSCRIPT: `tool.requested` gains `target` and `edits`, filled by the adapter, so tool and diff cards are drawn without reading a backend's tool input. A shape change, so `v` is bumped. PROTO-AGENT-EVENTS-R20 and R21 added.
- 2026-11-09: `user.message` for a message written during a turn is emitted when its own turn starts (PROTO-CLAUDE-CODE-STREAM-R43). No requirement IDs changed.
- 2026-11-08: from #272 inbox: a fork's `session.state` goes to `idle` when its backend process is ready, a new lifecycle fact. No requirement IDs changed.
- 2026-11-08: from #266 inbox: `question.answered` holds at most one answer per question; an open question on showing a question once, since its tool events still project. No requirement IDs changed.
- 2026-11-07: a backend's brief may name user entries it writes that project only to `backend.passthrough` (PROTO-CLAUDE-CODE-STREAM-R34). No requirement IDs changed.
- 2026-11-05: `question.answered` gains `declined`, recorded on Claude Code 2.1.316 (PROTO-CLAUDE-CODE-STREAM, Questions); R19 reworded to project a deny as declined.
- 2026-11-05: for FLOW-APPROVE-PERMISSION: Claude Code's question tool projects to `question.asked` in the adapter, which answers the open question on where questions come from; `question.asked` gains optional `header`, option `description`, and `takes_text`; an answer gains `own_words`, the user's own words moving to `value` with it; `answered_by` gains `backend` for a withdrawn question. A shape change, so `v` is bumped. PROTO-AGENT-EVENTS-R18 and R19 added.
- 2026-11-05: for FLOW-SEND-MESSAGE: `user.message` carries the message's `attachments`, a change of shape, so `v` is bumped with it; the Claude Code adapter's projection version changes when it projects them (PROTO-CLAUDE-CODE-STREAM-R26). No requirement IDs changed.
- 2026-11-04: from #136 inbox: PROTO-AGENT-EVENTS-R17 takes each message's highest count, not its latest, since Claude Code can send an `assistant` line with a lower partial count after a `message_delta` for the same message; synthetic `assistant` messages add nothing to the count.
- 2026-11-02: `usage.updated.turn_output_tokens` added for the working mark's token count beside the elapsed time; PROTO-AGENT-EVENTS-R17 added: the count belongs to one turn and takes each message's latest count. A shape change, so `v` is bumped. Elapsed time needs no field: it is the `turn.started` envelope's `ts` while the turn runs and `turn.completed.duration_ms` after.
- 2026-11-02: from #54 inbox: the open-elsewhere notices also precede a title appended to the record of a session Core is not running (PROTO-AGENT-EVENTS-R15 added), at `warning` for `session_open_elsewhere` and `info` for `session_open_check_failed` (R16 added). A Core-made notice lives only in the view open when it is emitted, by design. PROTO-AGENT-EVENTS-R14 is unchanged and is verified with Core's resume (M1).

- 2026-11-01: from #35 inbox: Opening a session added: when Core projects a session into a view, when it rebuilds one, and when it may drop one; `seq` counts within a view. Open question on what resume must settle for merging record and live input (R02's exception, matching prompts, partial text). No requirement IDs changed; R02 and R12 unchanged.

- 2026-11-01: from #34 inbox: `session.notice` gains `session_open_elsewhere` (with `pid`) and `session_open_check_failed`, emitted by Core's resume from the adapter's live-session check (PROTO-AGENT-EVENTS-R14 added); who wrote a record prompt decides its origin and whether it can title a session; the permission-decision open question names the recorded fields; open question on `peer` origins.

- 2026-11-01: aligned with ADR 0017 (supersedes ADR 0010): events come from the session record and the live stream, merged on the backend's message ids, and nothing projected is stored; the raw log is the live lines in both directions; Core emits lifecycle events itself and writes them to the app log; `source` names the input and message id in place of a raw-log range; projection cache removed. PROTO-AGENT-EVENTS-R11, R12 and R13 added; R01 to R10 unchanged in meaning.

- 2026-11-01: from #14 inbox: an Interface that meets an unknown `v` says it needs updating rather than asking Core to re-project. No requirement IDs changed.
- 2026-11-01: from M0-T6 inbox: `core` lifecycle records listed as built (start failed, Core restarted); a change to Core's projection of them bumps `v`.

- 2026-11-01: `usage.updated.turns` counts Ersa turns.

- 2026-11-01: `session.notice` added, so PROTO-CLAUDE-CODE-STREAM-R10's warning has an event. The schema stays at `v` 1, since no projection has been persisted yet.

- 2026-11-01: from M0-T2 inbox: `v` is the schema version and the cache also keys on each adapter's projection version; the raw log gains `core` lifecycle records from which `session.state` and `session.ended` project; tool data is open (R08 reworded to say so, meaning unchanged); passthrough `payload` may be a string; `question.answered` value shape stated.

- 2026-10-31: `ts` comes from the raw log, not the projection clock, so PROTO-AGENT-EVENTS-R03 is satisfiable; host-written lines are in the raw log and project to `user.message` and `permission.decided`. No requirement IDs changed.
- 2026-10-31: elaborated to ready: envelope, event vocabulary, projection rules, requirements.
- 2026-10-31: reframed as a projection of the raw log; commands moved to PROTO-CORE-API.
- 2026-10-31: created as draft.
