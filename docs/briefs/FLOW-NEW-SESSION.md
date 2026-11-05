---
id: FLOW-NEW-SESSION
type: flow
title: Start a new session
status: ready
references: [LAY-SHELL, PROTO-CORE-API, FLOW-RESUME-SESSION]
adrs: [0011, 0017, 0018]
---

# Start a new session

## Intent

A new session begins with the message, not with a form. The user chooses New session for a project and the transcript area becomes the new-session screen: the composer, focused, with the session controls beneath it saying where and how the session will run (host, project, workspace, backend, model, effort, permission mode; LAY-SHELL). Each control starts at what the user chose last time for that project, so the common case is to type and send. Nothing exists until the first message is sent: then Ersa makes the workspace, starts the backend in it, sends the message as the first turn, and the screen becomes the session's transcript with its tile in the rail.

A workspace is an isolated checkout by default (a git worktree for a git project). The user can instead run the session in the project's own folder, the root workspace, when isolation is not wanted: a quick question about the code, or a project whose version control Ersa has no workspace adapter for. Sessions in the root workspace share its files with each other and with the user.

Adding a project is the step before: the user names a folder on a host and says whether they trust it (ADR 0018). With no project yet, the new-session screen offers to add one.

## Requirements

- **FLOW-NEW-SESSION-R01**: When the user chooses New session for a project, Ersa shall show the new-session screen in place of the transcript, with the composer focused and the session controls showing the host, project, workspace, backend, model, effort and permission mode the session will start with.
- **FLOW-NEW-SESSION-R02**: Ersa shall start the session controls at the values the user last started a session with for that project; with none, at a new workspace for a git project and the root workspace otherwise, the first available backend on the project's host, that backend's default model and effort, and the permission mode in which the backend asks before every action outside its allow rules.
- **FLOW-NEW-SESSION-R03**: Ersa shall create no workspace and no session until the user sends the first message from the new-session screen; leaving the screen without sending shall leave nothing behind.
- **FLOW-NEW-SESSION-R04**: When the user sends the first message with a new workspace chosen, Ersa shall create an isolated workspace for the project, start a session in it with the chosen backend, model, effort and permission mode, and send the message as the session's first turn.
- **FLOW-NEW-SESSION-R05**: When the user sends the first message with the root workspace chosen, Ersa shall start the session in the project's root workspace, the project's own folder, with the chosen backend, model, effort and permission mode, and send the message as the session's first turn.
- **FLOW-NEW-SESSION-R06**: When the project's version control has no workspace adapter in Ersa, the session controls shall offer the root workspace only.
- **FLOW-NEW-SESSION-R07**: The session controls shall offer the backends detected on the project's host, and shall show a backend that is missing or failed detection with its status rather than hide it.
- **FLOW-NEW-SESSION-R08**: When the backend fails to start, Ersa shall show the failure on the new-session screen with the backend's own message, keep the user's message in the composer, and remove the session and any workspace it created for that session.
- ~~**FLOW-NEW-SESSION-R09**~~: retired 2026-11-05 (workspaces are not a level in the rail), replaced by R12.
- **FLOW-NEW-SESSION-R10**: When Ersa has no project, the new-session screen shall offer to add one, and adding a project shall ask whether the user trusts it before the project exists.
- **FLOW-NEW-SESSION-R11**: When the project's host cannot list its backends, the session controls shall say that detection failed, with the reason, rather than that no backend is available.
- **FLOW-NEW-SESSION-R12**: When the session has started, Ersa shall make it the active session, show its tile in the rail under its project, and show the session's values in the session controls.

## Verification

The replay server refuses every request that would create or change a session (PROTO-CORE-API, Conformance), so it cannot run R03 to R08, or R12. Those run against a real Core with a stand-in backend that can answer or fail to start; the Interface's own sequence is tested against a fake client.

## Built so far

A project is added by typing its folder path and answering the trust question. New session shows the new-session screen and creates nothing until the first message (R01 to R05, R08, R12). Until the open question on changing values mid-session is answered, the session controls under a running session show its values read-only. A host whose backend listing fails reads as having no backend available, which R11 replaces. The rail still groups sessions under a heading per workspace, so the place R12 gives a tile is still to build (LAY-SHELL). With no session chosen, the window shows the newest.

## Open questions

- Whether model, effort and permission mode can change during a session from the session controls, where the backend allows it, or only at start. Until answered, the controls show a running session's values read-only.
- The mode control (a named preset, milestone M3) and where it sits among the session controls.
- Which models and efforts a backend offers: the adapter's knowledge of its tested versions, or detected on the host.
- Which session a reopened window shows when several were running (FLOW-RESUME-SESSION).

## Changes

- 2026-11-05: R09 retired and R12 added: a started session's tile sits under its project, since workspaces are not a level in the rail (LAY-SHELL). This closes the open question on where a root workspace's sessions sit. Tests that cited R09 for the active session and the session controls cite R12; the test of workspace headings cites nothing until the rail's tree is built.
- 2026-11-02: from #55 inbox: R08 also removes the session, since a workspace with a session cannot be removed; R11 added for a host whose backend listing fails; Verification added (R03 to R09 run against a real Core, not the replay server); Built in M0 becomes Built so far, with the interim read-only controls and the root workspace's interim place in the rail.

- 2026-11-02: made ready from the owner's sketch: composer-first, session controls, nothing created before the first message, root workspace as the alternative to a worktree, failed start cleans up; FLOW-NEW-SESSION-R01 to R10 added. Open questions on backend detection and mode at start answered (detection; model, effort and permission mode at start, mode at M3).
- 2026-11-01: the user says whether they trust a project when adding it (ADR 0018).
- 2026-11-01: from #14 inbox: what M0 built; workspace reuse and the reopened window's session added as open questions.
- 2026-10-31: created as draft.
