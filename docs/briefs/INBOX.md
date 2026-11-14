# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 PROTO-AGENT-EVENTS: `tool.requested`'s `target` says what a call acts on but not whether it is a path, a command, a search or an address, and `TRANSCRIPT.md` cuts a path at its start and anything else at its end, while R08 makes only a path relative; the Interface may not read the tool's input to tell (Tool calls). Meanwhile #327 takes a target for a path when it lies inside the workspace folder with no space after the folder (a space there reads as a command's arguments, so a file in the workspace whose name has a space shows whole), or is spelled as an absolute one (a drive letter, a share, or `/` with no space), so a relative path such as a Glob pattern is cut at its end and a command that starts with `/usr/bin/…` but has no space would be cut at its start.
- [ ] 2026-11-14 none (docs/design/TRANSCRIPT.md, Tool cards): the open card holds the input and the output "each under a label", with no appearance for a call with no input fields or an empty output; a call like that opens to a label over nothing. Meanwhile #327 shows each label whatever it holds, as the page says, so a failed call with no output still says Error.
- [ ] 2026-11-14 FLOW-READ-TRANSCRIPT: no requirement says how a tool card shows a call the session ended without a result (stopped, or the backend exited mid-call). Meanwhile #327 keeps its running mark and stops its clock: the detail is empty once `session.ended` arrives.
- [ ] 2026-11-14 FLOW-READ-TRANSCRIPT: R06 runs a clock only "while it runs live", and a session opened while its backend works projects the record first (PROTO-AGENT-EVENTS), so a call already running when the user opens the session arrives from the record and shows no clock or progress until its result. Meanwhile #327 does that: a call counts as live by its `tool.requested`'s `source.input`.


## Resolved recently (optional, trimmed in planning passes)
