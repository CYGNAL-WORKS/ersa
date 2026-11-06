---
id: PROTO-CORE-API
type: protocol
title: Core request/response API
status: ready
references: [PROTO-AGENT-EVENTS]
adrs: [0011, 0017, 0018]
---

# Core request/response API

The protocol between the Interface (in the Shell) and the Core process: everything that is not an agent event. Requests from the Interface, responses from Core, and notifications from Core carrying agent events for subscribed sessions. The machine-readable form is `protocol/schema/core-api.schema.json` at the repository root, maintained by the implementing ticket; TypeScript types for the Interface are generated from it.

## Parties

One Core process per machine (the user's local machine, or a remote host reached through an SSH tunnel). One or more Interfaces attach to it. The replay server implements the same protocol over recorded fixtures.

## Transport and framing

A local stream socket: a named pipe on Windows (`\\.\pipe\ersa-core-<user SID>`, the SID because a user name comes from the environment), a Unix domain socket elsewhere (`$XDG_RUNTIME_DIR/ersa-core.sock` or equivalent). Only the current user may connect: the pipe's security descriptor grants the current user alone, and the socket lives in a directory only the user can enter. Before sending anything, a client verifies that the process serving the pipe or socket runs as the same user, and refuses to talk to it otherwise, so a squatter on the name never receives the user's requests. Lines are capped at a size Core states; an over-long or non-UTF-8 line gets a protocol error and the connection closes. A connection that sends no `core.hello` within a short deadline is closed. The endpoint is per user by default; `ersa-core serve` (and `replay`) also accept an explicit endpoint, so tests and a second Core never collide with the user's own. An explicit endpoint is a label (letters, digits, `-`, `_`) that Core appends to the user's endpoint name in the same private place, so it is exactly as private as the default; Core takes no raw pipe name or socket path, and no environment variable overrides the endpoint. Newline-delimited JSON-RPC 2.0: requests carry `id`, `method`, `params`; responses carry `id` and `result` or `error`; notifications carry `method` and `params` with no `id`. Either side may send notifications. Core writes nothing unsolicited except `session.event` for active subscriptions and, to every connection past `core.hello`, `session.tile`, `session.send_progress`, `host.status` and `core.notice`; a connection that cannot keep up is closed, except for `session.tile` and `session.send_progress`, which never close one (see Notifications).

## Handshake

The first request on a connection is `core.hello` with `{ client, client_version, api_versions: [..] }`. Core answers `{ core_version, api_version, event_projection_version, host: { id, name, platform }, capabilities: [..] }`. If no `api_version` overlaps, Core answers with an `invalid_state` error whose message and `detail` name the versions it supports, and closes.

## Methods

Names are `noun.verb`. Identifiers are strings Core issues. Listing methods return arrays of the full object; mutating methods return the affected object.

### Core

- `core.hello`: see Handshake.
- `core.status`: `{ sessions_running, sessions_waiting, uptime_s, db_path }`.
- `core.shutdown`: stops all sessions cleanly and exits. Interface disconnect alone never stops sessions (ADR 0011).

### Hosts

- `host.list`: the local host and configured remote hosts: `{ id, name, kind (local | ssh), status (connected | disconnected | unknown), backends: [{ name, version (optional, present when available), status (available | missing | error) }] }`.
- `host.add`, `host.remove`, `host.probe` (re-detect backends and VCS tools): remote hosts are milestone M4; the methods are reserved now so the Interface shape is stable.

### Projects

- `project.list`, `project.add { host_id, path, trusted }`, `project.set_trust { id, trusted }`, `project.remove { id }`: `{ id, host_id, path, name, vcs (git | perforce | none), trusted }`. `project.find { host_id, path }`: the project on that host whose folder is `path`, compared as `project.add` compares folders, or null. The Interface asks it before the trust question, so a folder that is already a project is chosen without one (FLOW-NEW-SESSION-R18). `project.add` for a folder that is already a project on that host is refused with `invalid_state`, and the error's `detail` is that project's id. `trusted` is the user's answer when adding the project, and only a trusted project's own configuration loads in its sessions (ADR 0018). `project.remove` is refused with `invalid_state` while the project has an isolated workspace or a session in its root workspace; it never cascades.

### Workspaces

- `workspace.list { project_id }`, `workspace.create { project_id, name (optional), base (optional) }`, `workspace.remove { id, force (optional) }` (refused with `invalid_state` while the workspace has sessions; it never cascades): `{ id, project_id, kind (root | isolated), path, vcs_ref, state (ready | creating | removing | error) }`. Every project has one root workspace, its own folder, which exists from `project.add` and is listed first; its `vcs_ref` is empty, since that checkout is the user's to change. `workspace.create` makes an isolated workspace (a git worktree for a git project) and is refused with `invalid_state` for a project whose version control has no workspace adapter. `workspace.remove` refuses the root workspace with `invalid_state`, and otherwise returns the workspace as it was claimed for removal, with `state: removing`. `base` names what an isolated workspace starts from: a branch from `branch.list`, or any other revision git can resolve (a tag, a commit, `main~1`); Core refuses one git can't resolve, and one that starts with `-`, with `invalid_params`, and passes it to git as a revision, never as an option. Resolving it has a 10-second limit, past which Core answers `vcs_error`. Without `base`, Core starts from the repository's default branch, as `branch.list` marks it, or from the checked-out commit when none is marked.
- `branch.list { project_id }`: the project's local branches, for choosing a new workspace's base branch: `[{ name, default }]`, sorted by name. At most one is `default`: the local branch `origin/HEAD` points at, else `main`, else `master`, else the branch the project's folder has checked out; never what is checked out while one of the others exists, and none on a detached checkout with none of the others, when `workspace.create` without `base` starts from the checked-out commit. Core gives its git call a 10-second limit and answers `vcs_error` when it runs out. Refused with `invalid_state` for a project whose version control has no workspace adapter, and with `vcs_error` and git's message when git fails.

### Sessions

- `session.list`: `{ id, workspace_id, host_id, backend, model, effort, permission_mode, mode, title, state, created_at, last_event_seq (absent before the first event), sending (absent unless a send is under way: its phase, as `session.send_progress` names it), tile: { state, context_tokens, context_window, cost_usd_total, pending: [{ kind (permission | question), request_id }] } }`. The `tile` object is what the rail renders; Core keeps it current from events. A session's `title` is its backend's: the title the backend's own tools list for it, which Ersa never stores. Its `created_at` is the time of the first entry in its session record, else the time of the app log's first fact for it. Its `mode` is Ersa's.
- `session.create { workspace_id, backend, model (optional), effort (optional), permission_mode (optional), mode (optional) }`: creates and starts; it takes no title, since the title is the backend's. `model`, `effort` and `permission_mode` are ids from `backend.list`; Core refuses any other id. A `model` or `effort` left out is left to the backend, and Core stores only what was named: `session.list` reports the named values and leaves the others absent, which means the backend's own default (a replayed session, and a session created before these fields existed, report none). The permission mode is the exception: the backend must always be told one (PROTO-CLAUDE-CODE-STREAM, Invocation), so a `permission_mode` left out becomes the backend's default mode, stored and reported as the mode the session started with. The backend's `default` marks in `backend.list` are the adapter's knowledge of what the backend chooses on its own, for the session controls to show, not a promise of what a given session ran with. `mode` is stored and shown; it takes effect from milestone M3, when modes exist.
- `session.start { id }`, `session.stop { id }`, `session.resume { id }`, `session.remove { id }`. Restarting a stopped session is `session.resume` (milestone M1); until then `session.start` on a stopped session answers `invalid_state` naming M1. `session.remove` is refused while the session is live.
- `session.send { id, text, attachments (optional) }`: sends a user message with its attachments; what it answers, and when, is under Turns and attachments, below. `text` may be empty when there are attachments. To a session whose backend is still starting, Core sends once it has started. To one whose backend failed to start, Core answers `invalid_state` with "the backend could not be started:" and the adapter's reason, including the backend's own error output, so the new-session screen can show it (FLOW-NEW-SESSION-R08).
- `session.interrupt { id }`: stops the turn the agent is working on; the turn ends with `turn.completed` and outcome `interrupted` (PROTO-AGENT-EVENTS). A message not yet handed to the backend is canceled instead (Turns and attachments). With neither, it does nothing and succeeds.
- `attachment.inspect { host_id, path }`: what the composer shows for a file, image or folder the user added by path, read on that host: `{ path, name, kind (file | image | folder), size_bytes (file and image), lines (optional: a text file's line count), media_type (image), target (optional: where a link points) }`. `not_found` when nothing is at the path, and `invalid_params` for anything but a regular file or a folder (a pipe, a device), or when its 30-second limit is reached. An image is a file whose first bytes are one of the image types the host's backends take; Core reads those bytes, not the name, to tell. Core counts lines only in a text file of at most 10 MB, and returns no content.
- `session.decide_permission { id, request_id, behavior (allow | deny), message (optional), updated_input (optional), apply_suggestions (optional array of indices) }`.
- `session.answer_question { id, request_id, answers }`.
- `session.set_title { id, title }`: renames the session in its backend, so the new title shows in the backend's own tools too (for Claude Code, `PROTO-CLAUDE-CODE-STREAM`, Session record). Ersa keeps no title of its own. Core trims the title, takes each run of spacing as one space, and refuses with `invalid_params` a title that is then blank, longer than 200 characters, or holding another control character; it answers `invalid_state` for a session its backend has not named yet (no backend session id, so no record) or whose record is gone. The single line of at most 200 characters is Claude Code's bound; it stays in Core until a second backend needs its own.
- `session.set_mode { id, mode }` (takes effect on next start).
- `session.subscribe { id, from_seq }`: Core sends every event with `seq >= from_seq` as `session.event` notifications, then continues live. Returns `{ view, last_seq, first_seq }`; `last_seq` is absent when the view has no events yet. A view holds a bounded amount of events (64 MiB); once it has let go of its earliest ones, `first_seq` is the first event it still holds, a subscription from before it starts there, and the Interface says that earlier events are no longer shown. `first_seq` is absent while the view holds every event from 0. `view` identifies the projection Core is serving (when Core rebuilds one: `PROTO-AGENT-EVENTS`, Opening a session), and changes whenever Core rebuilds it. Sequence numbers count within a view, so a client holding events from a different view discards them and subscribes again from 0, or from `first_seq`.
- `session.unsubscribe { id }`.
- `session.remove` also removes the session's scratch folder (Turns and attachments).
- `session.raw_log { id, from, to }`: the session's raw log for the developer pane, the lines exchanged with its backend since Core opened it (held in memory, never stored): `[{ index, ts, dir (backend | host), line }]`, at most 1000 per call; the client pages on from the last index plus one. Core's lifecycle facts are in the app log, not here.

### Turns and attachments

An attachment is a file, an image or a folder that goes with a message (FLOW-SEND-MESSAGE). `session.send` takes them as `attachments`, in the order the user added them:

- `{ kind: "image", path }`: an image file on the session's host, which Core reads.
- `{ kind: "image", media_type, data }`: an image the user pasted, as base64 `data` with its `media_type`.
- `{ kind: "file", path }` and `{ kind: "folder", path }`: a file or folder on the session's host, by absolute path.

Core checks every attachment before it sends anything, and refuses the whole message with an application error of `kind: attachment_refused` whose `data.index` is the place in `attachments` of the first one it can't send, and whose message says why:

- a path where nothing is, or whose name holds a control character;
- an image whose type or size the session's backend does not take (the adapter states its limits), or inline images that together exceed 10 MB, which leaves room under the line cap for the words (a request past the cap is still refused as a protocol error);
- a folder that holds the session's workspace;
- a copy of more than 100 MB or 10,000 entries, checked before copying and again while copying, so a file that grows meanwhile still stops the copy;
- a time limit reached (below), such as a read from a network drive that no longer answers.

An attachment that is a link is judged by its target and keeps its own name. An image goes to the backend inline, as an image the agent sees. A file or folder goes as its path relative to the session's workspace when its target is inside it. One outside it is copied first into the session's scratch folder, `.ersa/attachments/<session id>/<n>/` at the workspace's root, where `<n>` numbers the message's copies in order, the first one more than the highest number already in the session's scratch folder, so it is never reused, across restarts too, keeping its name, and goes as that copy's path. Each session has its own scratch folder, made by its own first copy, and no other session reads or empties it, so sessions that share a root workspace never touch each other's copies. A copied folder's links are left out, never followed. Core refuses the message when `.ersa`, `.ersa/.gitignore`, or any folder on the way to a copy is a link or otherwise not a real file or folder inside the workspace, and writes a `.gitignore` holding `*` into `.ersa/` whenever it has none, so version control ignores the folder. A copy is made aside, in the session's own `.partial/`, and moved into place only when whole; when any copy fails (a file gone or locked, the disk full, a time limit reached), Core removes what it copied for that message, including copies already moved into place, and refuses it with the reason, and the session's next send empties its `.partial/` first, so a crash leaves nothing behind for long. A session's copies stay while the session exists, and `session.remove` removes its scratch folder with it. Emptying and removing never follow a link: a link found there is removed itself, never what it points at.

Core takes the Interface's word that each path is one the user named (FLOW-SEND-MESSAGE-R13); it refuses nothing by where a path came from.

**Sending.** One message is sent at a time per session: a second `session.send` meanwhile is refused with `invalid_state`. A send passes through checking, copying, waiting for a starting backend, and handing the message to the backend; through all of them Core reports where it is as `session.send_progress` notifications (below), and `session.list` reports the session's `sending` phase while a send is under way, so an Interface that reconnects mid-send knows of it. Waiting for a starting backend is limited to 2 minutes and handing the message over to 30 seconds; past the first the message was not sent, past the second Core answers `delivery_unknown`, and either way the session can send again. All the file system work for one message, and each `attachment.inspect`, runs as one task Core can cancel, with a time limit of its own (5 minutes for a message, 30 seconds for an inspect; the Interface waits longer than each); when the limit is reached or the send is canceled, the task ends and nothing is left waiting on the file system. Checking and copying never hold up `session.interrupt` or an answer to a permission request or question on the same session. `session.send` answers only when its message has been handed over or will not be:

- `{ turn }`: the backend has the message. `turn` is absent when Core has not projected the message yet; the `user.message` event follows.
- An error: the message was not sent, and nothing it copied remains.
- An error with `kind: delivery_unknown`: Core handed the message over but can't tell whether the backend took it (the backend stopped mid-handover, say). The transcript shows it if it arrived.

`session.interrupt` before the message is handed over (while checking, copying, or waiting for the backend to start) cancels the send, which answers `invalid_state` saying the message was stopped. How the adapter puts the paths and images into the backend's message is the adapter's (for Claude Code, PROTO-CLAUDE-CODE-STREAM, User message). The `user.message` event for the turn carries the attachments as sent (PROTO-AGENT-EVENTS).

### Backends and modes

- `backend.list { host_id }`: `{ name, label, version (optional, present when available), status (available | missing | error), tested { min, max }, models: [{ id, label, default }], efforts: [{ id, label, default }], permission_modes: [{ id, label, default }] }`. The choices are the adapter's knowledge of its tested versions, not detected on the host; `label` is what the Interface shows (the backend's own, such as "Claude Code") and `default` marks the one a new session starts with.
- `mode.list`: `{ name, description }`; modes as presets are milestone M3; the method is reserved.

### Settings

- `settings.get`, `settings.set { patch }`: user settings as a JSON object, patch-merged (RFC 7386). `settings.get` returns only what the user has set; defaults are Core's and are not echoed. Known keys so far: `workspaces.root` (where workspaces are created; default under Core's data directory); `new_session.last_used.<project id>` (`{ host_id, project_id, workspace, backend, model, effort, permission_mode }`, the values that project last started a session with, which the Interface writes after a start and reads to fill the session controls, FLOW-NEW-SESSION-R02; a value left out is sent as null, so the merge clears it).

### Workbench (milestone M2, reserved)

- `fs.list { workspace_id, path }`, `fs.read { workspace_id, path, range (optional) }`, `fs.diff { workspace_id, base (optional) }`.
- `terminal.open { workspace_id }`, `terminal.input`, `terminal.resize`, `terminal.close`, with a `terminal.output` notification.

## Notifications (Core to Interface)

- `session.event { session_id, event }`: one `PROTO-AGENT-EVENTS` envelope.
- `session.send_progress { session_id, phase (checking | copying | starting | handing_over), copied_bytes (copying), total_bytes (copying) }`: where a send is, sent when its phase changes and at least every 5 seconds until it is answered, at most once a second. Coalesced like `session.tile`: a busy connection gets the newest per session, and it never closes a connection. The Interface waits for its send's answer for as long as these keep arriving.
- `session.tile { session_id, tile }`: the tile object changed (so the rail updates without subscribing to every session's events). A tile is the session's whole state, so tiles are coalesced: Core may project several backend lines at once and publish one tile for them, and a connection that is busy gets only the newest tile per session when it is ready, never a backlog. A client therefore sees some of the tiles a session passes through, in order, always ending on the current one; for tiles, PROTO-CORE-API-R09's same notifications means that.
- `host.status { host_id, status }`.
- `core.notice { level, message }`: for things the user should see that belong to no session.

## Errors

JSON-RPC error objects with `code` (standard JSON-RPC codes for protocol-level failures, including `-32602` for invalid params; `-32000` for application errors), `message`, and, on application errors only, `data: { kind, detail (optional), index (optional) }` where `kind` is one of `not_found`, `invalid_state`, `backend_missing`, `backend_not_logged_in`, `vcs_error`, `transport_error`, `permission`, `internal`, `attachment_refused` (with `index`, the refused attachment's place), `delivery_unknown`.

## Versioning

`api_version` negotiated in the handshake; additive changes (new methods, new optional fields) keep the version; removals or meaning changes bump it. Three changes bump it by one together, shipping with whichever of them ships first: `workspace.create` without `base` starts from the default branch, `session.send`'s `turn` becomes optional, and the error kinds `attachment_refused` and `delivery_unknown` are added. Reserved methods may return `not_implemented` (`kind: invalid_state`) until their milestone.

## Conformance

A conforming Core passes a request/response test suite driven over the socket, and the replay server passes the same suite with fixtures substituted for live backends. Generated TypeScript types compile against the schema.

The replay server answers read-only requests exactly as Core does. It never changes anything outside its own data: every request that would change a session, a project, a workspace or a setting answers `invalid_state` saying the session is a replay, so a replay cannot create worktrees in the user's repositories or alter their settings. A replayed session stays open and idle after its last recorded line until `core.shutdown`. Pacing defaults to the recorded timing; an instant pace is for tests. The replay server never serves the user's own endpoint: with no explicit endpoint it serves the label `replay`, so an Interface reaches a replay only when told to, and a replay never stands in for, or blocks, the user's Core. It applies Core's own limits to the recording, line size included, and a recording it cannot read faithfully is refused at start with the reason, never played with a default in its place.

A replay capture is what the replay server sent for one fixture, kept so the Interface's tests can replay it without running Core. It is a JSON object: `sessions` (`session.list` on attach), `sessions_after` (`session.list` once the replay has played out), `notifications` (every notification, in the order it arrived), and `ersa_commit` (the Ersa commit the capture was written from, ending `-dirty` when the tree had uncommitted changes outside the captures). A capture is checked against a fresh run of the replay server, which fails when the capture is stale. The check ignores `ersa_commit`, which says where a capture came from, not what it holds. Rewriting the captures builds Core first, so a capture never records a stale build.

## Requirements

- **PROTO-CORE-API-R01**: The first request on a connection shall be `core.hello`, and Core shall reject any other method before it with `invalid_state`.
- ~~**PROTO-CORE-API-R02**~~: retired 2026-11-01 (a view holds a bounded number of events), replaced by R14.
- **PROTO-CORE-API-R03**: Disconnecting all Interfaces shall not stop any session; only `session.stop` or `core.shutdown` shall.
- **PROTO-CORE-API-R04**: `session.decide_permission` shall cause the adapter to answer the backend's pending request with the given behavior, and a `permission.decided` event shall follow.
- **PROTO-CORE-API-R05**: `session.list` shall return each session's `tile` object reflecting the latest `usage.updated` and the set of pending permission and question requests.
- **PROTO-CORE-API-R06**: Every response and notification shall validate against `protocol/schema/core-api.schema.json`.
- **PROTO-CORE-API-R07**: When no `api_version` is shared, `core.hello` shall return an error naming the versions Core supports.
- **PROTO-CORE-API-R08**: `session.raw_log` shall return lines exactly as the backend emitted them, without their line terminator.
- **PROTO-CORE-API-R09**: The replay server shall accept the same requests and produce the same notifications as Core for a recorded session.
- **PROTO-CORE-API-R10**: A reserved method shall return an error with `kind: invalid_state` and a message naming the milestone, never a protocol-level error.
- **PROTO-CORE-API-R11**: A project shall be trusted only when `project.add` or `project.set_trust` said so, and its `trusted` value shall persist until `project.set_trust` changes it.
- **PROTO-CORE-API-R12**: `session.set_title` shall rename the session in its backend, so that the backend's own session listing and `session.list` both show the new title.
- **PROTO-CORE-API-R13**: `session.subscribe` shall return the `view` it serves, the same `view` for as long as Core keeps that projection and a different one whenever Core rebuilds it.
- ~~**PROTO-CORE-API-R15**~~: retired 2026-11-02 (a value not named is the backend's to choose, and Core does not know it), replaced by R16.
- **PROTO-CORE-API-R16**: `session.create` shall start the backend with the `model` and `effort` it was given, passing nothing for one left out, and with the `permission_mode` given or else the backend's default permission mode, which is always stated to the backend; `session.list` shall report the `model` and `effort` that were given, absent otherwise, and the `permission_mode` the session started with.
- **PROTO-CORE-API-R17**: Each replay capture shall equal what the replay server sends for its fixture, apart from `ersa_commit`.
- **PROTO-CORE-API-R18**: `branch.list` shall list the project's local branches and mark as `default` the one `origin/HEAD` points at, else `main`, else `master`, else the checked-out branch, and none on a detached checkout with none of those.
- **PROTO-CORE-API-R19**: `workspace.create` shall create the isolated workspace from its `base` when given one, refusing with `invalid_params` one git can't resolve, and otherwise from the branch `branch.list` marks default, else the checked-out commit.
- **PROTO-CORE-API-R20**: `project.add` for a folder already a project on that host shall be refused with `invalid_state` whose `detail` is the existing project's id, and shall add nothing.
- **PROTO-CORE-API-R21**: `session.send` shall send an image attachment to the backend inline as an image, and a file or folder attachment inside the session's workspace as its path relative to the workspace.
- **PROTO-CORE-API-R22**: `session.send` shall copy a file or folder attachment from outside the session's workspace into that session's own `.ersa/attachments/<session id>/<n>/` in the workspace, keeping its name and leaving out a folder's links, send the copy's relative path, and keep `.ersa/` ignored by version control; no other session's send shall read, write or empty that folder.
- **PROTO-CORE-API-R23**: When any attachment can't be sent or copied, `session.send` shall refuse the message with `attachment_refused`, the attachment's `index` and the reason, send nothing, and leave no copy behind.
- **PROTO-CORE-API-R24**: `attachment.inspect` shall report a path's kind, size and, for a text file of at most 10 MB, its line count, telling an image by its first bytes and refusing anything but a regular file or a folder.
- **PROTO-CORE-API-R25**: `session.interrupt` shall interrupt the session's running turn, which ends with outcome `interrupted`; while a message is not yet handed to the backend (being checked or copied, or waiting for the backend to start) it shall cancel that message, leaving no copy and sending nothing; otherwise it shall succeed without effect.
- **PROTO-CORE-API-R26**: `session.send` shall answer `{ turn }` once the backend has the message, an error only when the message was not sent, and `delivery_unknown` when Core handed it over but can't tell whether the backend took it; it shall send `session.send_progress` at each phase and at least every 5 seconds until it answers.
- **PROTO-CORE-API-R27**: `project.find` shall return the project on the host whose folder matches the path as `project.add` compares folders, else null.
- **PROTO-CORE-API-R28**: `session.remove` shall remove the session's scratch folder without following any link in it, and no copy in it shall be removed while the session exists, except the copies of a message whose send failed.
- **PROTO-CORE-API-R14**: `session.subscribe` with `from_seq` shall deliver every event from that sequence number onward, or from `first_seq` when the view no longer holds events before it, in order, with no gaps or duplicates, then continue live.

## Out of scope

- Authentication between Interface and Core on the local machine; the socket's OS permissions suffice. Remote Cores are reached through the user's SSH tunnel.
- Multi-user Cores.

## Open questions

- Whether Core should refuse attachments that are likely secrets (a key or credentials file) before copying them into a workspace.
- Whether `attachment.inspect` becomes part of the workbench's `fs` methods when they arrive (M2).
- Whether to allow more than one Interface attached at once (useful for a second window), and if so whether notifications fan out to all.
- Whether Core notifies an Interface when a project or session is created, and pushes `core.status` when its counts change; until then the window rereads `session.list` on a `session.tile` for an unknown session and reads the counts at attach.
- Whether the window still reads `core.status` at attach. Nothing in the window shows its counts now that the rail's foot is silent while all is well.

## Changes

- 2026-11-05: for FLOW-NEW-SESSION and FLOW-SEND-MESSAGE: `project.add` for an existing project's folder names it in `detail`; `branch.list` added and `workspace.create`'s `base` checked against it; `session.send` takes attachments (Turns and attachments), copied into the scratch folder from outside the workspace; `attachment.inspect` added; what `session.interrupt` does stated. PROTO-CORE-API-R18 to R25 added.
- 2026-11-05: from #173 merge review, before any of it shipped: each session copies into its own scratch folder, removed with the session; `project.find` added; `workspace.create`'s `base` takes any revision git resolves, and without one starts from the default branch, a change of meaning that, with `turn` becoming optional and the two new error kinds, bumps `api_version` by one, shipping with whichever ships first; a refused attachment is named by `index` (`attachment_refused`); `session.send` answers once the message is handed over, with `session.send_progress` while copying and `delivery_unknown` when Core can't tell; `session.interrupt` cancels a send not yet handed over; one cancelable task with its own limit per message and per inspect; limits on waiting for a starting backend and on handover; `sending` in `session.list`; links never followed when removing; `base` never read as an option. R19, R22, R23 and R25 reworded; R26 to R28 added.
- 2026-11-05: from #155 merge review: whether the window still reads `core.status` at attach is an open question. No requirement IDs changed.
- 2026-11-03: from #111 merge review: `session.tile` is coalesced (one tile per projected batch, the newest per session for a busy connection) and never closes a connection, as Core already did; what PROTO-CORE-API-R09 asks of tiles stated. No requirement IDs changed.
- 2026-11-02: from #55 inbox: `session.send` to a starting backend waits for it, and to one that failed to start answers `invalid_state` with the adapter's reason; settings key `new_session.last_used.<project id>` named. No requirement IDs changed.
- 2026-11-02: from #64 inbox: the replay capture format described under Conformance; the commit stamp is `ersa_commit`, since Core names the process, not a commit (PROTO-CORE-API-R17 added).

- 2026-11-02: US spelling (ADR 0020, docs/design/CONTENT.md). Meaning unchanged; no requirement IDs changed.
- 2026-11-02: from #43 inbox: `session.set_title`'s title bounds and its answers for a blank title, an unnamed session and a missing record. No requirement IDs changed.
- 2026-11-02: R16 reworded before it shipped: the permission mode is always stated to the backend, so an unnamed one is the backend's default, stored and reported; model and effort stay absent when not named.
- 2026-11-02: from #52 inbox: the root workspace's `vcs_ref` is empty; Core stores and reports only the named `model`, `effort` and `permission_mode`, leaving the rest to the backend (PROTO-CORE-API-R15 retired, R16 added); `backend.list`'s `tested { min, max }` replaces two fields, a removal, so `api_version` becomes 3 with this change.
- 2026-11-02: from #53 inbox: `backend.list` carries each backend's `label`. No requirement IDs changed.
- 2026-11-02: for FLOW-NEW-SESSION: every project has a root workspace (`kind`); `workspace.create` refused without a workspace adapter; `session.create` takes `model`, `effort` and `permission_mode`, reported by `session.list`; `backend.list` lists each backend's models, efforts and permission modes with labels and defaults. PROTO-CORE-API-R15 added.
- 2026-11-01: from #35 inbox: a view holds a bounded amount of events; `session.subscribe` returns `first_seq` once it has let go of its earliest. PROTO-CORE-API-R02 retired, R14 added.
- 2026-11-01: from #35 inbox: a session's title is its backend's: `session.set_title` renames through the backend (PROTO-CORE-API-R12 added) and `session.create` takes no title; `created_at` is the record's first entry, else the app log's first fact; `session.subscribe` returns `view`, and a client holding events from another view starts over from 0 (PROTO-CORE-API-R13 added); `session.reproject` removed, which bumps `api_version`; the bump ships with the method's removal in code.
- 2026-11-01: project trust (ADR 0018): `trusted` on projects, `project.add` takes it, `project.set_trust` added; PROTO-CORE-API-R11 added.
- 2026-11-01: aligned with ADR 0017: `session.raw_log` serves the in-memory raw log, `dir` no longer includes `core` (lifecycle facts go to the app log); `session.reproject` withdrawn. No requirement IDs changed.

- 2026-11-01: from #14 inbox: notifications for created projects and sessions, and a pushed `core.status`, added as an open question.
- 2026-11-01: from M0-T7 inbox: the replay server refuses every mutating request with `invalid_state`, keeps a replayed session idle after its last line until `core.shutdown`, and paces by recorded timing by default; from review of PR #27: it never serves the user's own endpoint (default label `replay`), applies Core's line limit, and refuses a recording it cannot read faithfully. No requirement IDs changed.

- 2026-10-31: from #18 inbox and review: an explicit endpoint is a label beside the user's own endpoint, never a raw name or path, and nothing in the environment overrides it; on Windows a test stands two labeled endpoints in for "explicit beside default". No requirement IDs changed.

- 2026-11-01: from M0-T6 inbox and review: unsolicited notifications defined; tile carries `state`; `mode` until M3; start, remove and reproject rules; `raw_log` paging; R08 says the line terminator is not part of a line (meaning unchanged: a line never included it). No requirement IDs added or retired.

- 2026-11-01: `serve` and `replay` accept an explicit endpoint (from M0-T8).

- 2026-11-01: the pipe name uses the user's SID; clients verify the server runs as the same user; line cap and handshake deadline; the security-descriptor open question closed.

- 2026-11-01: from M0-T5 inbox: backend status values; `data` only on application errors; version mismatch is `invalid_state`; `settings.get` returns user-set values only, `workspaces.root` named; sequence numbers absent before the first event; `workspace.remove` and `project.remove` behavior stated. No requirement IDs changed.

- 2026-11-01: `dir` includes `core` for Core's lifecycle records.
- 2026-10-31: `session.raw_log` entries carry `dir`, since the raw log records host-written lines too.
- 2026-10-31: elaborated to ready: transport, handshake, methods, notifications, errors, requirements.
- 2026-10-31: created as draft.
