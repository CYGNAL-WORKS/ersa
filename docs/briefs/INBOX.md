# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-02 FLOW-CLOSE-AND-QUIT: on Linux, ending a Core that will not stop (R06) ends Core's process group, but each backend runs in a process group of its own, so a backend's tree outlives a Core ended there (on Windows, job objects end the backends with Core); on macOS the Shell cannot yet find the process behind Core's socket, so Quit asks Core to stop but cannot end it. Built Windows-first meanwhile (#32).
- [ ] 2026-11-02 FLOW-CLOSE-AND-QUIT-R04: "working or waiting on the user" read as tile states `running`, `starting` and `waiting_user`; an idle session stops at Quit without being counted. Built that way meanwhile (#32).

## Resolved recently (optional, trimmed in planning passes)
