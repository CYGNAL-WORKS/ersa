---
id: FLOW-NEW-SESSION
type: flow
title: Start a new session
status: ready
references: [LAY-SHELL, PROTO-CORE-API, FLOW-RESUME-SESSION, FLOW-SEND-MESSAGE]
adrs: [0011, 0017, 0028]
---

# Start a new session

## Intent

A new session begins with the message, not with a form. The user chooses New session for a project and the transcript area becomes the new-session screen: the composer, focused, with the session controls beneath it saying where and how the session will run (host, project, workspace, backend, model, effort, permission mode; LAY-SHELL). Each control but the base branch starts at what the user chose last time for that project, so the common case is to type and send. Nothing exists until the first message is sent: then Ersa makes the workspace, starts the backend in it, sends the message as the first turn, and the screen becomes the session's transcript with its tile in the rail.

The session controls lead with where the session runs, three chips fixed once it starts: host, project and workspace. The workspace is an isolated checkout by default (a git worktree for a git project), made from a base branch the user picks, which starts at the repository's default branch, not at whatever the project's folder has checked out (unless the repository has no other). The user can instead run the session in the project's own folder, the root workspace, when isolation is not wanted: a quick question about the code, or a project whose version control Ersa has no workspace adapter for. Sessions in the root workspace share its files with each other and with the user.

The project chip on the new-session screen is the only place a project is added; the rail has no way to add one. Its list ends with Add project, which on the local host opens the system's folder dialog, then asks whether the user trusts the folder (ADR 0028), then chooses the new project. The trust question reads like the Claude Code CLI's own folder-trust prompt, which users already know, and has two answers: cancel, which it offers first, or trust. A project is never added untrusted; the user withdraws trust later from the project heading's More menu (LAY-SHELL). Nobody types a folder path. With no project yet, the project chip offers to choose a folder, and sending opens the same dialog, keeping the message.

## Requirements

- **FLOW-NEW-SESSION-R01**: When the user chooses New session for a project, Ersa shall show the new-session screen in place of the transcript, with the composer focused and the session controls showing the host, project, workspace, backend, model, effort and permission mode the session will start with.
- **FLOW-NEW-SESSION-R02**: Ersa shall start the session controls at the values the user last started a session with for that project; with none, at a new workspace for a git project and the root workspace otherwise, the first available backend on the project's host, that backend's default model and effort, and the permission mode in which the backend asks before every action outside its allow rules.
- **FLOW-NEW-SESSION-R03**: Ersa shall create no workspace and no session until the user sends the first message from the new-session screen; leaving the screen without sending shall leave nothing behind.
- **FLOW-NEW-SESSION-R04**: When the user sends the first message with a new workspace chosen, Ersa shall create an isolated workspace for the project, start a session in it with the chosen backend, model, effort and permission mode, and send the message as the session's first turn.
- **FLOW-NEW-SESSION-R05**: When the user sends the first message with the root workspace chosen, Ersa shall start the session in the project's root workspace, the project's own folder, with the chosen backend, model, effort and permission mode, and send the message as the session's first turn.
- **FLOW-NEW-SESSION-R06**: When the project's version control has no workspace adapter in Ersa, the session controls shall offer the root workspace only.
- **FLOW-NEW-SESSION-R07**: The session controls shall offer the backends detected on the project's host, and shall show a backend that is missing or failed detection with its status rather than hide it.
- **FLOW-NEW-SESSION-R08**: When the backend fails to start, Ersa shall show the failure on the new-session screen with the backend's own message, keep the user's message in the composer, and remove the session and any workspace it created for that session.
- ~~**FLOW-NEW-SESSION-R09**~~: retired 2026-11-05 (workspaces are not a level in the rail), replaced by R12 and R13.
- ~~**FLOW-NEW-SESSION-R10**~~: retired 2026-11-05 (a project is added from the project chip, by the folder dialog), replaced by R16 to R20.
- **FLOW-NEW-SESSION-R11**: When the project's host cannot list its backends, the session controls shall say that detection failed, with the reason, rather than that no backend is available.
- **FLOW-NEW-SESSION-R12**: When the session has started, Ersa shall make it the active session and show the session's values in the session controls.
- **FLOW-NEW-SESSION-R13**: When the session has started, Ersa shall show its tile in the rail where the rail's arrangement puts it (FLOW-ORGANIZE-SESSIONS).
- **FLOW-NEW-SESSION-R14**: The project chip shall list the projects on the chosen host, most recently used first (by its newest session), each with its name, its folder's path, its kind (Git or Folder) and, when it is not trusted, that it is not trusted, with the chosen project marked, and shall end the list with Add project.
- **FLOW-NEW-SESSION-R15**: When the user opens the session controls for no particular project (New session at the top of the rail, or no active session), Ersa shall choose the host's most recently used project.
- **FLOW-NEW-SESSION-R16**: When the user chooses Add project, or presses Ctrl+O on the new-session screen, with the local host chosen, Ersa shall open the system's folder dialog; once the user picks a folder that is not yet a project, Ersa shall ask whether they trust it, saying what trust lets run, that it covers every workspace of the project, and that it lasts until withdrawn, then add the project as their answer and R25 say and choose it in the session controls.
- **FLOW-NEW-SESSION-R17**: When the user cancels the folder dialog or the trust question, Ersa shall add no project and leave the session controls and the composer as they were.
- **FLOW-NEW-SESSION-R18**: When the folder the user picks is already a project on that host, Ersa shall find it before asking anything and choose that project without the trust question.
- **FLOW-NEW-SESSION-R19**: When the local host is chosen and has no projects, the project chip shall offer to choose a folder, and sending a message shall open the folder dialog as R16 does and keep the message in the composer; once the project is added it is chosen, and the message waits for the user to send it.
- **FLOW-NEW-SESSION-R20**: Ersa shall offer no field for typing a project's folder path.
- **FLOW-NEW-SESSION-R21**: When a git project has a new workspace chosen, the workspace chip shall offer the project's local branches as the base branch, with a search box that narrows them, and shall start at the repository's default branch for every new session, or, when the repository has none, at the checked-out commit; R04 shall create the workspace from the chosen base branch.
- **FLOW-NEW-SESSION-R22**: The workspace chip shall name the workspace a session will start in: a new workspace with its base branch, or the project folder.
- **FLOW-NEW-SESSION-R23**: While the backends on the chosen host are being detected, the new-session screen shall say so above the composer, and the composer shall be unavailable until detection ends.
- **FLOW-NEW-SESSION-R24**: The trust question shall show the folder's path, ask whether it is a project the user created or one they trust (their own code, a well-known open source project, or their team's work) and to review the folder first if not, and say that agents will be able to read, edit and run files there.
- **FLOW-NEW-SESSION-R25**: The trust question shall offer two answers, Cancel first and then Trust, with keyboard focus on Cancel when it opens; Trust shall add the project as trusted, and Ersa shall offer no way to add a project that is not trusted.

## Verification

The replay server refuses every request that would create or change a session (PROTO-CORE-API, Conformance), so it cannot run R03 to R08, R12, R13 or the adding of a project in R16, R18 and R19. Those run against a real Core with a stand-in backend that can answer or fail to start; the Interface's own sequence is tested against a fake client, with the system's folder dialog replaced by a stand-in that answers with a folder or a cancel.

## Built so far

A project is added by typing its folder path in the rail and answering the trust question; R14 to R20 replace that. New session shows the new-session screen and creates nothing until the first message (R01 to R05, R08, R12). Until the open question on changing values mid-session is answered, the session controls under a running session show its values read-only. A host whose backend listing fails reads as having no backend available, which R11 replaces. The rail still groups sessions under a heading per workspace, so R13 is still to build (LAY-SHELL). Workspaces are made from the project folder's checked-out branch until R21 is built. With no session chosen, the window shows the newest. The trust question still offers to add a project untrusted, as ADR 0018 had it; R24 and R25 are still to build.

## Open questions

- Whether model, effort and permission mode can change during a session from the session controls, where the backend allows it, or only at start. Until answered, the controls show a running session's values read-only.
- The mode control (a named preset, milestone M3) and where it sits among the session controls.
- Which models and efforts a backend offers: the adapter's knowledge of its tested versions, or detected on the host.
- Which session a reopened window shows when several were running (FLOW-RESUME-SESSION).
- How the user picks a folder on a remote host, which the system's folder dialog cannot browse (FLOW-REMOTE-SESSION).
- Whether the base branch can also be a remote branch or a tag, and whether a project remembers a base branch other than the default.
- What the trust question's security guide link opens, as the CLI's prompt has one. Ersa has no user guide yet, and the CLI's guide is one backend's, so the question has no link until one exists.

## Changes

- 2026-11-06: owner's decision to match the Claude Code CLI's folder-trust prompt (ADR 0028, superseding 0018): the trust question shows the folder's path, a safety check and what agents can do there (R24), and its answers are Cancel, first and focused, and Trust; a project is no longer added untrusted (R25). R16 reworded to name R25, its meaning otherwise kept. Open question on the security guide link.
- 2026-11-05: R13 reworded for the rail's arrangement (FLOW-ORGANIZE-SESSIONS): a new tile sits where the arrangement puts it, under its project by default. Meaning unchanged for the default arrangement.
- 2026-11-05: from #173 merge review, before any of it shipped: an existing project is found before the trust question (R18), the trust question says what trust covers (R16), and the no-projects offer is the local host's (R19).
- 2026-11-05: owner's decisions on where a session runs: a project is added from the project chip through the system's folder dialog, and no path is typed (R10 retired; R14 to R20 added); the project chip lists projects most recently used first with path, kind and trust; a new workspace is made from a base branch that starts at the repository's default branch (R21, R22); the screen says when backends are being detected (R23). Open questions on a remote host's folder picker and on other kinds of base.
- 2026-11-05: R09 retired; R12 and R13 added. R12 keeps the active session and the session controls, and the tests that cited R09 for them cite it. R13 puts a started session's tile under its project, since workspaces are not a level in the rail (LAY-SHELL); it has no test until the rail's tree is built, and the test of workspace headings cites nothing meanwhile. This closes the open question on where a root workspace's sessions sit.
- 2026-11-02: from #55 inbox: R08 also removes the session, since a workspace with a session cannot be removed; R11 added for a host whose backend listing fails; Verification added (R03 to R09 run against a real Core, not the replay server); Built in M0 becomes Built so far, with the interim read-only controls and the root workspace's interim place in the rail.

- 2026-11-02: made ready from the owner's sketch: composer-first, session controls, nothing created before the first message, root workspace as the alternative to a worktree, failed start cleans up; FLOW-NEW-SESSION-R01 to R10 added. Open questions on backend detection and mode at start answered (detection; model, effort and permission mode at start, mode at M3).
- 2026-11-01: the user says whether they trust a project when adding it (ADR 0018).
- 2026-11-01: from #14 inbox: what M0 built; workspace reuse and the reopened window's session added as open questions.
- 2026-10-31: created as draft.
