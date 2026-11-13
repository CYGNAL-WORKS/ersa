---
id: FLOW-CLOSE-AND-QUIT
type: flow
title: Close the window, quit Ersa
status: ready
references: [LAY-SHELL, PROTO-CORE-API, FLOW-RESUME-SESSION]
adrs: [0011, 0035]
---

# Close the window, quit Ersa

## Intent

Closing the window and quitting are different acts. Closing hides the window to the tray icon: Core and every session keep running, so an agent can finish its work with the window out of the way. Quit, from the tray icon's menu, stops Ersa entirely: Core stops every session and exits, and nothing of Ersa is left running. A Shell that exits any other way (a crash, an update, a kill) leaves Core and the sessions running, and the next launch attaches to them.

## Requirements

- **FLOW-CLOSE-AND-QUIT-R01**: When the user closes the window, Ersa shall hide it, keep the tray icon, and leave Core and every session running.
- **FLOW-CLOSE-AND-QUIT-R02**: When the user opens Ersa from the tray icon, or launches Ersa while it is already running, Ersa shall show the existing window, attached to the same Core, and shall not start a second Shell.
- **FLOW-CLOSE-AND-QUIT-R03**: When the user chooses Quit from the tray icon's menu, Ersa shall ask Core to shut down (`core.shutdown`) and exit only after Core has stopped.
- **FLOW-CLOSE-AND-QUIT-R04**: When the user chooses Quit while any session is working or waiting on the user (a tile state of starting, running or waiting on the user; an idle session is not counted), Ersa shall first ask the user to confirm, saying how many sessions will stop; when the user cancels, Ersa shall leave the window, Core and every session as they were.
- **FLOW-CLOSE-AND-QUIT-R05**: When Core shuts down for a quit, every session that was running shall end with reason `user_stopped`, and no backend process it started shall remain.
- **FLOW-CLOSE-AND-QUIT-R06**: When Core has not stopped within 10 seconds of a quit, Ersa shall end Core's process tree and exit.
- **FLOW-CLOSE-AND-QUIT-R07**: When the Shell exits without a quit, Core and every session shall keep running, and the next launch shall attach to that Core.
- **FLOW-CLOSE-AND-QUIT-R08**: A release build shall serve its Core on the endpoint label `daily` with its data in the `daily` folder of Ersa's application data folder, launch that Core itself and shut it down on quit; a debug build shall refuse to attach to the label `daily`.

## Development

A debug build attached to a labeled endpoint (LAY-SHELL, Development) quits without shutting that Core down: it was started by someone else, such as a replay or a test. The daily build (ADR 0035) is a Shell of its own: one daily window and one development window may run at once, each the only one of its build, each with its own Core; quitting the daily build shuts down its Core.

## Open questions

- R06 beyond Windows: on Linux, ending Core's process group leaves each backend's own process group running, so a backend's tree outlives a Core ended that way (on Windows, job objects end the backends with Core); on macOS the Shell cannot yet find the process behind Core's socket, so Quit asks Core to stop but cannot end it. Windows first (ADR 0006); the other platforms need a rule before they are built.
- Whether the first close tells the user once that Ersa is still running in the tray.
- The macOS equivalent: menu bar icon, and whether the Dock icon stays.
- Whether Ersa starts at login, hidden in the tray.
- How the quit question words a quit that failed. Today it shows the Shell's reasons after "Ersa did not quit:", and those still name Core; they wait for the rest of the copy outside the rail (#159).

## Changes

- 2026-11-13: the daily build runs beside a development build, each with its own Shell and Core (ADR 0035). FLOW-CLOSE-AND-QUIT-R08 added.
- 2026-11-05: from #155 inbox: the wording of a failed quit's reason is an open question. No requirement IDs changed.
- 2026-11-02: US spelling (ADR 0020, docs/design/CONTENT.md). Meaning unchanged; no requirement IDs changed.
- 2026-11-02: from #32 inbox: R04 says which sessions count (starting, running, waiting on the user; meaning unchanged); the limits of R06 on Linux and macOS recorded as an open question.
- 2026-11-01: created ready, from the owner's decision that the window closes to the tray and Quit stops Core.
