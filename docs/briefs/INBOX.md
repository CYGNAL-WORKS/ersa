# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-10-31 PROTO-CORE-API: `session.tile` must reach clients that have not subscribed, but Transport and framing says Core writes nothing unsolicited except for active subscriptions. Meanwhile every connection gets `session.tile` once `core.hello` succeeds, and one that cannot keep up is closed.
- [ ] 2026-10-31 PROTO-CORE-API: silent on `workspace.remove` for a workspace with sessions, on `session.start` for a stopped session, and on `session.reproject` for a running one. Meanwhile workspace removal is refused with `invalid_state` until its sessions are removed (never cascades), `session.start` on a stopped session answers `invalid_state` naming M1 (restarting is resume), and re-projecting a running session is refused (it is always on the current version).
- [ ] 2026-10-31 PROTO-CORE-API: `session.raw_log` has no size bound. Meanwhile one call returns at most 1000 lines and the client pages from the last index plus one.
- [ ] 2026-10-31 PROTO-CORE-API: `session.create`'s `mode` has no meaning before modes exist (M3). Meanwhile it is stored and shown, and every session starts in the backend's default permission mode.
- [ ] 2026-10-31 PROTO-AGENT-EVENTS: the lifecycle records listed (started, exited, stopped by the user, disconnected) do not cover a failed start or a Core that died while the session ran. Meanwhile Core also writes `start_failed` (carrying the exit code when the process already exited, so `session.ended` stays last) and `core_restarted`, which ends the session with `session.state ended` and `session.ended` reason `error`; "stopped by the user" is a field of `process_exited`, so one line yields both final events.
- [ ] 2026-10-31 PROTO-AGENT-EVENTS: the cache key (session, `v`, adapter projection version) has no part for Core's own projection of `core` lines, so a change there would not re-derive cached sessions. Meanwhile any such change must bump `v`.
- [ ] 2026-10-31 ARCH-CORE: no bound on one line from a backend. Meanwhile Core's transport caps a stdout or stderr line at 64 MiB; a process that writes past it is killed with its tree, and the session ends with a `backend_exited` error.

## Resolved recently (optional, trimmed in planning passes)
