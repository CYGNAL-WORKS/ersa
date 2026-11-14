---
id: FLOW-FORK-SESSION
type: flow
title: Fork a session
status: ready
references: [LAY-SHELL, PROTO-CORE-API, PROTO-CLAUDE-CODE-STREAM]
adrs: [0017]
---

# Fork a session

## Outcome

The user has a new session that carries another session's conversation up to its last finished reply and goes on from there on its own, while the original is unchanged.

## Trigger

The user chooses **Fork** in a tile's menu (`LAY-SHELL`, The rail).

## Steps

1. On `LAY-SHELL`, the user opens a session's tile menu. Fork is available unless a reply is running, a request waits on the user, or a message or a fork of the session is on its way; Core forks only a session whose conversation ends with a finished reply.
2. The user chooses Fork. A new tile appears where the rail's arrangement puts it and becomes the active session; its transcript holds the conversation up to that reply, and the composer is ready.
3. A fork of a session in an isolated workspace runs in a new isolated workspace of its own, on a new branch that starts from the commit the original's workspace has checked out; uncommitted changes stay with the original. A fork of a session in a root workspace runs in that root workspace. Either way it has the original's backend, model, effort and permission mode, in the original's section, not pinned. Its title is the original's followed by ` (fork)`, and none for a fork of an untitled session. It can be renamed before its first message while it runs.
4. The original session keeps its state: a running original keeps running.

## Variations and errors

- **A reply is running**, a request waits on the user, or a message is on its way: Fork is unavailable, and says it is available once the reply finishes.
- **A fork of the session is on its way**: Fork is unavailable, and says it is forking.
- **No reply has finished yet**: Fork stays offered; when chosen, Ersa says the fork was refused and why (no finished reply yet).
- **The last turn was interrupted**, or Ersa stopped mid-turn: Fork stays offered; when chosen, Ersa says the fork was refused and why (no finished reply).
- **The session is open in another process**, such as the backend's own terminal: Fork stays offered; when chosen, Ersa says the fork was refused and where the session is open.
- **The backend can't fork** (the backend, or the version installed, has no way to): Fork stays offered; when chosen, Ersa says the backend can't fork.
- **The fork fails to start**: Ersa says so with the backend's reason, as a failed start does (FLOW-NEW-SESSION), and the original is unchanged. The fork's tile stays, showing why, until the user deletes it, which removes a workspace and branch made for it.
- **The new workspace can't be made**: Ersa says so with the reason, no fork is made, and the original is unchanged.
- **The backend refuses the fork's name**: the fork has no title until it has its own record, then the title the backend copied from the original, until the user renames it.
- **Ersa restarts before a fork's first message**: the fork has no title, since the backend keeps no record of it until then.
- **A fork closed before its first message and resumed later** is forked from its original again (FLOW-RESUME-SESSION-R10), so the agent's conversation in it includes what the original did since, though Ersa shows the fork up to its fork point until its first message.
- **A root workspace**: the fork and the original share files, as two sessions in one workspace always do; deleting either keeps the workspace, as deleting a session always keeps a root workspace (FLOW-ARCHIVE-DELETE-SESSION).

## Requirements

- **FLOW-FORK-SESSION-R01**: When the user forks a session, Ersa shall start a new session whose conversation is the original's up to its last finished reply, under a new backend session id, leaving the original's record and state unchanged.
- ~~**FLOW-FORK-SESSION-R02**~~: retired 2026-11-08 (a fork of a session in an isolated workspace gets its own), replaced by R05.
- **FLOW-FORK-SESSION-R03**: Ersa shall offer Fork as unavailable, with the reason, while the session's listing shows a reply running, a request waiting on the user or a message on its way, or while Ersa has a fork of it on its way; Core shall refuse `session.fork` with `invalid_state` unless the session's conversation, as its record holds it, ends with a finished reply and no process has the session open elsewhere, and Ersa shall say why once Fork is chosen.
- **FLOW-FORK-SESSION-R04**: When a fork has started, Ersa shall make it the active session.
- **FLOW-FORK-SESSION-R05**: A fork of a session in an isolated workspace shall run in a new isolated workspace on a new branch starting from the commit the original's workspace has checked out, and a fork of a session in a root workspace shall run in that root workspace; either way it shall have the original's backend, model, effort, permission mode and section, and shall not be pinned or archived.
- ~~**FLOW-FORK-SESSION-R06**~~: retired 2026-11-09 (Ersa stores no title for a fork, so the title is the backend's), replaced by R07.
- ~~**FLOW-FORK-SESSION-R07**~~: retired 2026-11-09 (Core holds no title for a fork), replaced by R08.
- **FLOW-FORK-SESSION-R08**: When it starts a titled original's fork, Ersa shall have its backend name the fork with the original's title, shortened to fit the title limit, followed by ` (fork)`; it shall neither store nor hold a title for a fork, and shall show only what the backend has announced or the fork's record gives. A fork of an untitled session shall have no title until its backend gives one.
- **FLOW-FORK-SESSION-R09**: Core shall keep a fork's link to its original's record only until the fork has a record of its own, and shall then clear it.

## Out of scope

- Forking from an earlier reply than the last finished one.
- Bringing the original's uncommitted changes into the fork.

## Open questions

- Whether a fork resumed before its first message can keep its fork point: the backend keeps no record of the fork until then, so Ersa forks the original again as it is, and needs a way to copy the original only up to a named message.
- Whether Claude Desktop offers Fork during a reply and on an archived session. Neither has been observed, and the variations above stand until one has.

## Changes

- 2026-11-13: from #353 inbox: a fork closed before its first message and resumed later is forked from its original again, so it holds the original's later turns; whether it can keep its fork point is an open question. No requirement IDs changed.
- 2026-11-13: from #302 inbox: the menu marks Fork unavailable only for what the session list already shows (a reply running, a request waiting, a message or fork on its way); a missing finished reply, a session open elsewhere and a backend that can't fork are said after the click, from Core's refusal reason, since `session.list` carrying fork availability would have Core read every record and probe other processes on each listing. The words per reason come with PROTO-CORE-API-R80. R03 and Step 1 reworded.
- 2026-11-09: owner's decision from the audit against the backend rule: the backend names a fork at launch and Core holds no title (R07 retired for R08); a fork's link to its original goes once it has its own record (R09).
- 2026-11-09: from #272 inbox, owner's decision: Ersa names a fork in its backend and stores no title for it, so the two cases where the backend's title is not the marked one are variations. R06 retired for R07.
- 2026-11-08: owner's decision: a fork does what Claude Desktop's does (`claude-desktop/fork-2026-11-08.txt`). A fork of a session in an isolated workspace gets its own workspace and branch, and a fork's title is marked ` (fork)`. FLOW-FORK-SESSION-R02 retired for R05; R06 added.
- 2026-11-08: from #272 inbox: a fork shows the original's title, unmarked, until its own record gives one, and can be renamed before its first message; the open question narrows to whether Ersa should mark the name. No requirement IDs changed.
- 2026-11-07: open question on how a fork is named. No requirement IDs changed.
- 2026-11-05: created from the owner's decisions of 2026-11-05: Fork starts a new session from the last finished reply, unavailable with the reason while a reply runs or before one has finished. R01 to R04 added.
