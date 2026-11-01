---
id: PROTO-AGENT-EVENTS
type: protocol
title: Agent event protocol
status: ready
references: []
adrs: [0004, 0010, 0011, 0014]
---

# Agent event protocol

Ersa's own vocabulary for what happens in a session, independent of any backend. Every event is a projection of one or more raw log lines (ADR 0010): adapters provide the projection function, Core applies it and caches the result, and the Interface renders events. Commands in the other direction (send a message, decide a permission, interrupt) are not events; they are `PROTO-CORE-API` requests.

The machine-readable form is `protocol/schema/agent-events.schema.json` at the repository root, maintained by the implementing ticket with conformance tests against the fixtures. This brief defines the vocabulary the schema must express.

## Parties

Core produces events (by projecting raw lines) and streams them to subscribed Interfaces as `PROTO-CORE-API` notifications. Adapters produce the projection but never talk to the Interface.

## Envelope

Every event is an object with:

- `v`: event schema version, an integer. Bumped when any event's meaning or shape changes. Each adapter also versions its own projection logic; a cached projection is discarded and re-derived when either `v` or that adapter's projection version changes.
- `session_id`: Ersa's session identifier (not the backend's).
- `seq`: a monotonically increasing integer within the session, assigned by Core, gap-free. Used for resume (`from_seq`) and ordering.
- `ts`: ISO 8601 time the raw log recorded the last line the event came from (`raw_to`). Taken from the raw log, never from the clock at projection time, so re-projection yields the same value.
- `source`: `{ backend, backend_version, raw_from, raw_to }`: which backend and version produced the underlying lines, and the inclusive raw-log line range they came from.
- `event`: the typed payload; `event.type` discriminates.

## Event types

Names are `family.verb`. Fields listed are required unless marked optional.

### Session

- `session.started`: `backend`, `backend_version`, `model`, `cwd`, `capabilities` (string list), `backend_session_id` (optional). Projected from the backend's first announcement (Claude Code `system/init`).
- `session.state`: `state` one of `starting`, `running`, `waiting_user`, `idle`, `disconnected`, `ended`, `error`; `reason` (optional short string). Projected from Core's lifecycle records in the raw log and from backend lines (a pending permission request means `waiting_user`).
- `session.ended`: `reason` one of `user_stopped`, `backend_exited`, `error`; `exit_code` (optional).
- `session.notice`: `level` one of `info`, `warning`; `code` one of `backend_version_untested`; `message`. Something the user should know that is not an error, such as a backend newer than the newest version Ersa was tested against.

### Turn

- `turn.started`: `turn` (1-based index), `trigger` one of `user`, `resume`, `system`.
- `turn.completed`: `turn`, `outcome` one of `success`, `error`, `interrupted`, `max_turns`; `duration_ms`, `is_error`, `final_text` (optional).

### User

- `user.message`: `text`, `origin` one of `human`, `peer_session`, `system`; `peer` (optional: `{ session_id, name }`). Projected from a replayed user message, or recorded by Core when it sends one.

### Assistant

- `assistant.text`: `block_id`, `text`, `final` (boolean). With partial streaming, several events share a `block_id` with growing `text` and `final: false`, then one with `final: true`. Without partial streaming, one event with `final: true`.
- `assistant.thinking`: `block_id`, `summary` (optional text), `final`. Present even when the backend withholds content, so the Interface can show that thinking happened.

### Tool

- `tool.requested`: `tool_use_id`, `name`, `display_name` (optional), `input` (object), `parent_tool_use_id` (optional, for subagent trees).
- `tool.result`: `tool_use_id`, `content` (string or structured), `is_error`, `duration_ms` (optional).
- `tool.progress`: `tool_use_id`, `detail` (optional string). Projected from backend progress and task-summary lines.

### Permission

- `permission.requested`: `request_id`, `tool_use_id`, `tool_name`, `display_name` (optional), `description` (optional), `input` (object), `blocked_path` (optional), `suggestions` (array of `{ kind, label, payload }` where `kind` is `allow_rule`, `allow_directory`, or `set_mode` and `payload` is backend-specific and opaque to the Interface), `flags` (object of booleans: `default_to_no`, `suppress_always_allow`, `requires_user_interaction`, `classifier_approvable`), `reason` (optional: `{ kind, text }`).
- `permission.decided`: `request_id`, `behavior` one of `allow`, `deny`; `decided_by` one of `user`, `rule`, `backend`, `timeout`; `message` (optional); `applied_suggestions` (optional array of suggestion indices).
- `permission.denied_unanswered`: `tool_use_id`, `tool_name`, `message`. The backend denied because nobody could answer (observed when the control channel is not set up). Should not occur in a correctly configured session; its presence is a defect signal.

### Question

- `question.asked`: `request_id`, `questions` (array of `{ id, text, options (optional array of { id, label }), multi_select (boolean) }`).
- `question.answered`: `request_id`, `answers` (array of `{ id, value }`, where `value` is a string for free text or a single option id, or an array of option ids for multi-select), `answered_by` one of `user`, `timeout`.

### Usage

- `usage.updated`: `context_tokens` (optional), `context_window` (optional), `cost_usd_total` (optional), `turns` (optional: the number of Ersa turns started in the session, not the backend's API round trips), `rate_limit` (optional: `{ status, resets_at (optional), detail (optional) }`). Any field absent means "no new information"; the Interface keeps the last known value per field. Projected from result usage, rate-limit events, and thinking-token estimates.

### Error

- `error`: `code` one of `authentication_failed`, `backend_exited`, `protocol`, `backend_error`, `transport`; `message`; `recoverable` (boolean); `action` (optional: `log_in`, `restart`, `reconnect`).

### Passthrough

- `backend.passthrough`: `backend`, `kind` (the backend's own type and subtype joined with `/`), `payload` (the raw object, or the verbatim line as a string when it is not valid JSON). Every raw line that maps to no other event maps to exactly one of these. The Interface shows them only in the developer pane.

## Projection rules

- The raw log holds lines in three directions, each tagged: `backend` (what the backend emitted), `host` (what Ersa wrote to the backend), and `core` (lifecycle records Core writes itself: process started, start failed, process exited with its code and whether the user stopped it, and Core restarted while the session was live, which ends it with reason `error`). Host lines project too (a user message to `user.message`, a permission answer to `permission.decided`), and `core` lines project to `session.state` and `session.ended`, so re-projection reproduces the whole stream.
- Every raw line yields at least one event (possibly only `backend.passthrough`); no line is dropped.
- A raw line may yield several events (a Claude Code `result` yields `turn.completed` and `usage.updated`).
- Events never contain backend-specific field names outside `backend.passthrough.payload` and `permission.requested.suggestions[].payload`. Tool data is not backend protocol: `tool.requested.input`, `permission.requested.input`, and structured `tool.result.content` carry the tool's own arguments and output as the tool defines them.
- Projection is a pure function of the raw log and the adapter version; re-running it over the same raw log yields the same events with the same `seq`.

## Sequencing

Within a session: `session.started` first; `turn.started` before any `assistant.*`, `tool.*`, or `permission.*` of that turn; `permission.decided` after its `permission.requested`; `tool.result` after its `tool.requested`; `turn.completed` after everything in the turn; `session.ended` last. `usage.updated`, `session.state`, `error`, and `backend.passthrough` may appear anywhere.

## Errors and recovery

A projection failure on one raw line yields an `error` event with `code: protocol`, `recoverable: true`, plus a `backend.passthrough` of the offending line, and projection continues. Projection never aborts a session.

## Versioning

`v` on every event. The schema file carries the same version. The cache key is the session, `v`, and the adapter's projection version. Core's own projection of `core` lines is part of the event schema's meaning, so any change to it bumps `v`. The Interface refuses to render a `v` it does not know and says that the window is older than Core and needs updating. It does not ask Core to re-project: Core projects only at its own `v`, so re-projection cannot help an Interface older than Core.

## Conformance

The implementing crate projects every fixture in `docs/briefs/protocol/fixtures/claude-code/*/` and the result validates against the schema; a golden file per fixture records the expected event list and is reviewed when it changes.

## Requirements

- **PROTO-AGENT-EVENTS-R01**: Every event shall carry `v`, `session_id`, `seq`, `ts`, `source`, and `event`, and `seq` shall be gap-free and increasing within a session.
- **PROTO-AGENT-EVENTS-R02**: Every raw log line shall project to at least one event, with `backend.passthrough` as the fallback.
- **PROTO-AGENT-EVENTS-R03**: Projecting the same raw log twice with the same adapter version shall yield identical events.
- **PROTO-AGENT-EVENTS-R04**: A Claude Code `can_use_tool` control request shall project to one `permission.requested` carrying tool name, input, description, blocked path, suggestions, and flags.
- **PROTO-AGENT-EVENTS-R05**: A Claude Code `result` line shall project to one `turn.completed` and one `usage.updated` carrying total cost and token usage.
- **PROTO-AGENT-EVENTS-R06**: A Claude Code `authentication_failed` synthetic message shall project to one `error` with `code: authentication_failed` and `action: log_in`, and no `assistant.text`.
- **PROTO-AGENT-EVENTS-R07**: With partial streaming enabled, `assistant.text` events for one block shall share a `block_id` and end with exactly one `final: true`.
- **PROTO-AGENT-EVENTS-R08**: No event outside `backend.passthrough.payload` and `permission.requested.suggestions[].payload` shall contain a backend-specific field name; tool-defined data in `tool.requested.input`, `permission.requested.input`, and `tool.result.content` is not backend protocol.
- **PROTO-AGENT-EVENTS-R09**: Every projected event shall validate against `protocol/schema/agent-events.schema.json`.
- **PROTO-AGENT-EVENTS-R10**: A projection failure on a line shall yield a recoverable `error` event and continue with the next line.

## Out of scope

- Commands from the Interface (see `PROTO-CORE-API`).
- Rendering decisions; this protocol says what happened, not how it looks.

## Open questions

- Whether `question.*` is projected from a backend tool call (Claude Code's AskUserQuestion arrives as a tool use plus permission request) or needs backend-specific handling in the adapter; first implementation decides and records it.
- Whether subagent trees get their own `session_id` or stay nested under `parent_tool_use_id`.

## Changes

- 2026-11-01: from #14 inbox: an Interface that meets an unknown `v` says it needs updating rather than asking Core to re-project. No requirement IDs changed.
- 2026-11-01: from M0-T6 inbox: `core` lifecycle records listed as built (start failed, Core restarted); a change to Core's projection of them bumps `v`.

- 2026-11-01: `usage.updated.turns` counts Ersa turns.

- 2026-11-01: `session.notice` added, so PROTO-CLAUDE-CODE-STREAM-R10's warning has an event. The schema stays at `v` 1, since no projection has been persisted yet.

- 2026-11-01: from M0-T2 inbox: `v` is the schema version and the cache also keys on each adapter's projection version; the raw log gains `core` lifecycle records from which `session.state` and `session.ended` project; tool data is open (R08 reworded to say so, meaning unchanged); passthrough `payload` may be a string; `question.answered` value shape stated.

- 2026-10-31: `ts` comes from the raw log, not the projection clock, so PROTO-AGENT-EVENTS-R03 is satisfiable; host-written lines are in the raw log and project to `user.message` and `permission.decided`. No requirement IDs changed.
- 2026-10-31: elaborated to ready: envelope, event vocabulary, projection rules, requirements.
- 2026-10-31: reframed as a projection of the raw log; commands moved to PROTO-CORE-API.
- 2026-10-31: created as draft.
