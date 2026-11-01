# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-01 LAY-SHELL: the window reaches a replay (or any labelled endpoint) only when told to: a debug build with `ERSA_ATTACH=<label>` attaches to that label and never launches Core there, a release build ignores the variable, and the Core tile then reads "Attached to <label>" (#14). LAY-SHELL should state this, or choose another mechanism.
- [ ] 2026-11-01 FLOW-NEW-SESSION: M0 built the least that reaches a session (#14): a project is added by typing its folder path; "New session" on a git project creates a new worktree workspace and a `claude-code` session in it, every time; non-git projects offer no session; a workspace whose `session.create` fails is left behind. With no session chosen, the window shows the newest, so reopening shows the one that was running. The brief should decide workspace reuse, backend choice, and which session a reopened window shows.
- [ ] 2026-11-01 FLOW-APPROVE-PERMISSION: until the shelf (M1), a permission request is answered in the transcript with Allow or Deny only: no suggestions, no deny reason (#14). The flow should say which of `suggestions`, `message` and `updated_input` the shelf offers.
- [ ] 2026-11-01 PROTO-CORE-API: nothing tells an Interface that a project or session was created, or that `core.status` counts changed; the window rereads `session.list` on a `session.tile` for an unknown session and shows the status counts read at attach (#14). Consider a notification for created sessions and projects, and whether `core.status` should be pushed.
- [ ] 2026-11-01 none (DESIGN.md): there is no reading-measure token; the transcript's column is `calc(var(--size-rail) * 3)` meanwhile (#14). Proposal: a `size.measure` token.

## Resolved recently (optional, trimmed in planning passes)
