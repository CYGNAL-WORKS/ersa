---
id: PROTO-CORE-API
type: protocol
title: Core request/response API
status: ready
references: [PROTO-AGENT-EVENTS]
adrs: [0011]
---

# Core request/response API

The protocol between the Interface (in the Shell) and the Core process: everything that is not an agent event. Requests from the Interface, responses from Core, and notifications from Core carrying agent events for subscribed sessions. The machine-readable form is `protocol/schema/core-api.schema.json` at the repository root, maintained by the implementing ticket; TypeScript types for the Interface are generated from it.

## Parties

One Core process per machine (the user's local machine, or a remote host reached through an SSH tunnel). One or more Interfaces attach to it. The replay server implements the same protocol over recorded fixtures.

## Transport and framing

A local stream socket: a named pipe on Windows (`\\.\pipe\ersa-core-<user>`), a Unix domain socket elsewhere (`$XDG_RUNTIME_DIR/ersa-core.sock` or equivalent). Newline-delimited JSON-RPC 2.0: requests carry `id`, `method`, `params`; responses carry `id` and `result` or `error`; notifications carry `method` and `params` with no `id`. Either side may send notifications. Core writes nothing unsolicited except notifications for active subscriptions.

## Handshake

The first request on a connection is `core.hello` with `{ client, client_version, api_versions: [..] }`. Core answers `{ core_version, api_version, event_projection_version, host: { id, name, platform }, capabilities: [..] }`. If no `api_version` overlaps, Core answers with an error and closes.

## Methods

Names are `noun.verb`. Identifiers are strings Core issues. Listing methods return arrays of the full object; mutating methods return the affected object.

### Core

- `core.hello`: see Handshake.
- `core.status`: `{ sessions_running, sessions_waiting, uptime_s, db_path }`.
- `core.shutdown`: stops all sessions cleanly and exits. Interface disconnect alone never stops sessions (ADR 0011).

### Hosts

- `host.list`: the local host and configured remote hosts: `{ id, name, kind (local | ssh), status (connected | disconnected | unknown), backends: [{ name, version, status }] }`.
- `host.add`, `host.remove`, `host.probe` (re-detect backends and VCS tools): remote hosts are milestone M4; the methods are reserved now so the Interface shape is stable.

### Projects

- `project.list`, `project.add { host_id, path }`, `project.remove { id }`: `{ id, host_id, path, name, vcs (git | perforce | none) }`.

### Workspaces

- `workspace.list { project_id }`, `workspace.create { project_id, name (optional), base (optional) }`, `workspace.remove { id, force (optional) }`: `{ id, project_id, path, vcs_ref, state (ready | creating | removing | error) }`.

### Sessions

- `session.list`: `{ id, workspace_id, host_id, backend, mode, title, state, created_at, last_event_seq, tile: { context_tokens, context_window, cost_usd_total, pending: [{ kind (permission | question), request_id }] } }`. The `tile` object is what the rail renders; Core keeps it current from events.
- `session.create { workspace_id, backend, mode (optional), title (optional) }`: creates and starts.
- `session.start { id }`, `session.stop { id }`, `session.resume { id }`, `session.remove { id }`.
- `session.send { id, text }`: sends a user message; returns `{ turn }`.
- `session.interrupt { id }`.
- `session.decide_permission { id, request_id, behavior (allow | deny), message (optional), updated_input (optional), apply_suggestions (optional array of indices) }`.
- `session.answer_question { id, request_id, answers }`.
- `session.set_title { id, title }`, `session.set_mode { id, mode }` (takes effect on next start).
- `session.subscribe { id, from_seq }`: Core sends every event with `seq >= from_seq` as `session.event` notifications, then continues live. Returns `{ last_seq }`.
- `session.unsubscribe { id }`.
- `session.raw_log { id, from, to }`: raw lines for the developer pane: `[{ index, ts, dir (backend | host | core), line }]`.
- `session.reproject { id }`: discards the cached projection and re-derives; used after a projection version change.

### Backends and modes

- `backend.list { host_id }`: installed backends with versions and min/max tested versions.
- `mode.list`: `{ name, description }`; modes as presets are milestone M3; the method is reserved.

### Settings

- `settings.get`, `settings.set { patch }`: user settings as a JSON object, patch-merged.

### Workbench (milestone M2, reserved)

- `fs.list { workspace_id, path }`, `fs.read { workspace_id, path, range (optional) }`, `fs.diff { workspace_id, base (optional) }`.
- `terminal.open { workspace_id }`, `terminal.input`, `terminal.resize`, `terminal.close`, with a `terminal.output` notification.

## Notifications (Core to Interface)

- `session.event { session_id, event }`: one `PROTO-AGENT-EVENTS` envelope.
- `session.tile { session_id, tile }`: the tile object changed (so the rail updates without subscribing to every session's events).
- `host.status { host_id, status }`.
- `core.notice { level, message }`: for things the user should see that belong to no session.

## Errors

JSON-RPC error objects with `code` (standard JSON-RPC codes for protocol-level failures; `-32000` for application errors), `message`, and `data: { kind, detail (optional) }` where `kind` is one of `not_found`, `invalid_state`, `backend_missing`, `backend_not_logged_in`, `vcs_error`, `transport_error`, `permission`, `internal`.

## Versioning

`api_version` negotiated in the handshake; additive changes (new methods, new optional fields) keep the version; removals or meaning changes bump it. Reserved methods may return `not_implemented` (`kind: invalid_state`) until their milestone.

## Conformance

A conforming Core passes a request/response test suite driven over the socket, and the replay server passes the same suite with fixtures substituted for live backends. Generated TypeScript types compile against the schema.

## Requirements

- **PROTO-CORE-API-R01**: The first request on a connection shall be `core.hello`, and Core shall reject any other method before it with `invalid_state`.
- **PROTO-CORE-API-R02**: `session.subscribe` with `from_seq` shall deliver every event from that sequence number onward, in order, with no gaps or duplicates, then continue live.
- **PROTO-CORE-API-R03**: Disconnecting all Interfaces shall not stop any session; only `session.stop` or `core.shutdown` shall.
- **PROTO-CORE-API-R04**: `session.decide_permission` shall cause the adapter to answer the backend's pending request with the given behavior, and a `permission.decided` event shall follow.
- **PROTO-CORE-API-R05**: `session.list` shall return each session's `tile` object reflecting the latest `usage.updated` and the set of pending permission and question requests.
- **PROTO-CORE-API-R06**: Every response and notification shall validate against `protocol/schema/core-api.schema.json`.
- **PROTO-CORE-API-R07**: When no `api_version` is shared, `core.hello` shall return an error naming the versions Core supports.
- **PROTO-CORE-API-R08**: `session.raw_log` shall return lines exactly as the backend emitted them.
- **PROTO-CORE-API-R09**: The replay server shall accept the same requests and produce the same notifications as Core for a recorded session.
- **PROTO-CORE-API-R10**: A reserved method shall return an error with `kind: invalid_state` and a message naming the milestone, never a protocol-level error.

## Out of scope

- Authentication between Interface and Core on the local machine; the socket's OS permissions suffice. Remote Cores are reached through the user's SSH tunnel.
- Multi-user Cores.

## Open questions

- Whether to allow more than one Interface attached at once (useful for a second window), and if so whether notifications fan out to all.
- Named pipe security descriptor on Windows: current user only.

## Changes

- 2026-11-01: `dir` includes `core` for Core's lifecycle records.
- 2026-10-31: `session.raw_log` entries carry `dir`, since the raw log records host-written lines too.
- 2026-10-31: elaborated to ready: transport, handshake, methods, notifications, errors, requirements.
- 2026-10-31: created as draft.
