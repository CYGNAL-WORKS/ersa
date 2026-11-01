---
id: FLOW-CLOSE-AND-QUIT
type: flow
title: Close the window, quit Ersa
status: ready
references: [LAY-SHELL, PROTO-CORE-API, FLOW-RESUME-SESSION]
adrs: [0011]
---

# Close the window, quit Ersa

## Intent

Closing the window and quitting are different acts. Closing hides the window to the tray icon: Core and every session keep running, so an agent can finish its work with the window out of the way. Quit, from the tray icon's menu, stops Ersa entirely: Core stops every session and exits, and nothing of Ersa is left running. A Shell that exits any other way (a crash, an update, a kill) leaves Core and the sessions running, and the next launch attaches to them.

## Requirements

- **FLOW-CLOSE-AND-QUIT-R01**: When the user closes the window, Ersa shall hide it, keep the tray icon, and leave Core and every session running.
- **FLOW-CLOSE-AND-QUIT-R02**: When the user opens Ersa from the tray icon, or launches Ersa while it is already running, Ersa shall show the existing window, attached to the same Core, and shall not start a second Shell.
- **FLOW-CLOSE-AND-QUIT-R03**: When the user chooses Quit from the tray icon's menu, Ersa shall ask Core to shut down (`core.shutdown`) and exit only after Core has stopped.
- **FLOW-CLOSE-AND-QUIT-R04**: When the user chooses Quit while any session is working or waiting on the user, Ersa shall first ask the user to confirm, saying how many sessions will stop; when the user cancels, Ersa shall leave the window, Core and every session as they were.
- **FLOW-CLOSE-AND-QUIT-R05**: When Core shuts down for a quit, every session that was running shall end with reason `user_stopped`, and no backend process it started shall remain.
- **FLOW-CLOSE-AND-QUIT-R06**: When Core has not stopped within 10 seconds of a quit, Ersa shall end Core's process tree and exit.
- **FLOW-CLOSE-AND-QUIT-R07**: When the Shell exits without a quit, Core and every session shall keep running, and the next launch shall attach to that Core.

## Development

A debug build attached to a labelled endpoint (LAY-SHELL, Development) quits without shutting that Core down: it was started by someone else, such as a replay or a test.

## Open questions

- Whether the first close tells the user once that Ersa is still running in the tray.
- The macOS equivalent: menu bar icon, and whether the Dock icon stays.
- Whether Ersa starts at login, hidden in the tray.

## Changes

- 2026-11-01: created ready, from the owner's decision that the window closes to the tray and Quit stops Core.
