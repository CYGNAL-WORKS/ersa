# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-02 PROTO-CORE-API: `session.send` to a session whose backend failed to start answered only "the session is not running", so the new-session screen could not show the backend's own message (FLOW-NEW-SESSION-R08). Core now answers `invalid_state` with "the backend could not be started: <the adapter's reason, with the CLI's stderr>"; the brief should say so under Sessions.
- [ ] 2026-11-02 PROTO-CORE-API: the values each project last started a session with (FLOW-NEW-SESSION-R02) are kept in Core's settings under `new_session.last_used.<project id>` (host_id, project_id, workspace, backend, model, effort, permission_mode), so every window sees them; the brief's known settings keys should name it.
- [ ] 2026-11-02 FLOW-NEW-SESSION: R08 names only the workspace, but `workspace.remove` refuses a workspace that has a session, so after a failed start the Interface stops and removes the session too (for the root workspace as well), leaving nothing behind as R03 intends. The brief could say the session goes as well.
- [ ] 2026-11-02 LAY-SHELL: the rail's sessions now sit under a heading per workspace (project name, then branch or "Root workspace"), inside the existing sessions list, not yet the projects > workspaces > sessions tree the brief draws; the tree, and where a root workspace's sessions sit (an open question in FLOW-NEW-SESSION), want a rail ticket.
- [ ] 2026-11-02 FLOW-NEW-SESSION: the session controls under a running session show its values read-only, since whether they can change mid-session is an open question; and a host whose `backend.list` fails is shown as having no backend available. Decide both when the open question is answered.
- [ ] 2026-11-02 FLOW-NEW-SESSION: the ticket's seam "against the replay server" cannot run R03 to R09: the replay server refuses every request that would create or change a session. They are verified against a real Core with a stand-in backend instead.

## Resolved recently (optional, trimmed in planning passes)
