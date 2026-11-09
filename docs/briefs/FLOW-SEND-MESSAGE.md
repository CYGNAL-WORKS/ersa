---
id: FLOW-SEND-MESSAGE
type: flow
title: Write to a session
status: ready
references: [LAY-SHELL, PROTO-CORE-API]
adrs: [0011, 0017]
---

# Write to a session

## Intent

The user writes to the agent in the composer, on the new-session screen and on every session, and can add files, images and folders to a message. A message goes as one turn; from the moment it is sent until the turn ends, the composer's send becomes stop, which cancels a message that hasn't reached the agent and otherwise interrupts the turn. While the agent works, Enter still sends: the message waits in the backend and runs when the turn ends, as in Claude Desktop, and stop leaves it waiting. When a message can't be sent, the composer says why above the box and keeps the message, so nothing the user wrote is lost. When Ersa can't reach the sessions, the composer says so and waits.

An attachment is a file, an image or a folder that goes with a message. The user adds one from the composer's attach menu (files, images, or a folder, each through the system's dialog), by pasting an image, or by dropping files, images and folders on the box. Attachments sit in the box above the words, each with its name and a detail (an image's or file's size, a text file's line count, or that it is a folder), and each can be removed before sending. An image goes to the backend as an image the agent sees. A file or folder goes as its path, which the agent reads itself; one from outside the session's workspace is copied into the session's scratch folder in the workspace first, so the agent reads it where it works (PROTO-CORE-API, Turns and attachments). The transcript shows a sent message's attachments with it.

`docs/design/COMPOSER.md` says how the composer looks in each state.

## Requirements

- **FLOW-SEND-MESSAGE-R01**: When the user presses Enter, or chooses send, with words or an attachment in the composer, Ersa shall send them to the active session as one message and clear the composer once the session has accepted it; Shift+Enter shall start a new line.
- **FLOW-SEND-MESSAGE-R02**: Anything the user writes or attaches while a message is being sent shall stay in the composer after that message is accepted.
- **FLOW-SEND-MESSAGE-R03**: From the moment the user sends until the agent's turn ends, the composer shall offer stop in place of send; choosing stop before the message reaches the agent shall cancel the send and keep the message in the composer, and choosing it after shall interrupt the turn.
- ~~**FLOW-SEND-MESSAGE-R04**~~: retired 2026-11-09 (a message written while the agent works is sent and waits), replaced by R18.
- **FLOW-SEND-MESSAGE-R05**: When a message is refused, Ersa shall show the reason above the composer's box, in the user's terms, and keep the message and its attachments in the composer until the user sends again or changes it.
- **FLOW-SEND-MESSAGE-R06**: When Ersa can't reach the sessions, the composer shall say so above its box and be unavailable, keeping what the user had written, until Ersa reaches them again.
- **FLOW-SEND-MESSAGE-R07**: The composer's attach menu shall offer adding files, adding images and adding a folder, each through the system's dialog, and shall say that attachments can also be pasted or dropped on the box.
- **FLOW-SEND-MESSAGE-R08**: When the user pastes an image into the composer, Ersa shall attach it; when the user drops files, images or folders on the composer's box, Ersa shall attach each of them.
- **FLOW-SEND-MESSAGE-R09**: Each attachment shall show its name and its detail: the size of an image, the line count of a text file and the size of any other file, or that it is a folder; and each shall offer to remove it before the message is sent.
- **FLOW-SEND-MESSAGE-R10**: When an attachment is one Ersa can't send (an image of a kind or size the backend does not take, a folder too large to copy, or a path that no longer exists), Ersa shall say why beside that attachment and send nothing until the user removes it.
- **FLOW-SEND-MESSAGE-R11**: The transcript shall show each message's attachments with it: an image as an image, a file or folder by its name.
- **FLOW-SEND-MESSAGE-R12**: Adding the same file or folder twice to one message shall attach it once.
- **FLOW-SEND-MESSAGE-R13**: Ersa shall attach only a path the user named: one picked in the system's dialog or dropped on the box.
- ~~**FLOW-SEND-MESSAGE-R14**~~: retired 2026-11-07 (a file or folder is judged by its target), replaced by R17.
- **FLOW-SEND-MESSAGE-R15**: When Ersa can't tell whether a message reached the agent (the session was lost mid-send, or the send got no answer), the composer shall say so above its box and keep the message, and shall not say it failed.
- **FLOW-SEND-MESSAGE-R16**: While a message is on its way to the agent, the composer shall show where it is: how far its attachments are copied, or that the backend is starting.
- **FLOW-SEND-MESSAGE-R17**: Before the message is sent, a file or folder whose target is outside the session's workspace shall say that it will be copied into the workspace, and where.
- **FLOW-SEND-MESSAGE-R18**: While the agent is working, Enter with words or an attachment in the composer shall send them as one message, which waits and runs when the turn ends, and the composer shall clear once the session has accepted it.
- **FLOW-SEND-MESSAGE-R19**: While a sent message waits for the turn to end, the status line above the composer's box shall say so and show its first words, until its turn starts.
- **FLOW-SEND-MESSAGE-R20**: When the user chooses stop while a message waits, Ersa shall interrupt only the running turn, and the waiting message shall run next.

## Verification

The replay server refuses `session.send` and `session.interrupt` (PROTO-CORE-API, Conformance), so R01, R03 and R05 run against a fake client in the Interface's tests, and end to end against a real Core with a stand-in backend. The system's dialogs and the clipboard are replaced by stand-ins in the Interface's tests; dropping and pasting in the running app is an owner check.

## Built so far

The composer sends words with Enter, keeps what was typed meanwhile (R01, R02), and shows a refused send's reason beneath its box. Stop, attachments and the states above the box are still to build.

## Open questions

- Dictation: whether the composer takes speech, and through what (the owner's mockup draws a microphone beside the session controls).
- Whether a block of text the user writes that looks like Ersa's attachment list can be told from Ersa's own (PROTO-CLAUDE-CODE-STREAM, User message).
- Whether Ersa should refuse attachments that are likely secrets before copying them (PROTO-CORE-API).
- Whether pasting files copied in the system's file manager attaches them, where the webview gives their contents but not their paths.
- How the user sends Enter's newline and send on a keyboard without Shift, and whether Ctrl+Enter sends.

## Changes

- 2026-11-09: owner's decision from the audit against Claude Desktop: a message written while the agent works is sent and waits in the backend (R04 retired for R18), says so while it waits (R19), and stays waiting through stop (R20). The open question on queueing is closed.
- 2026-11-07: R14 retired for R17: a file or folder is judged by its target, as Core reports it in `attachment.inspect`'s `copied` (PROTO-CORE-API-R44), so a link inside the workspace whose target is outside it gets the note.
- 2026-11-05: from #173 merge review, before any of it shipped: stop is offered from the moment of sending and cancels a send that hasn't reached the agent (R03 reworded); a refused attachment is marked itself (R10); an outside attachment says where it will be copied (R14); a send whose outcome Ersa can't tell says so (R15); a copy shows its progress (R16); refusing likely secrets is an open question.
- 2026-11-05: created ready from the owner's composer mockup and decisions: send and stop, a refused message kept with its reason, the composer unavailable while Ersa can't reach the sessions, and attachments (the attach menu, paste and drop, removable, images inline and files as paths, copied into the scratch folder from outside the workspace). FLOW-SEND-MESSAGE-R01 to R14 added; R01 and R02 describe what is built.
