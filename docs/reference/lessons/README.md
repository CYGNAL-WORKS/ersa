# Lessons for later

Lessons about areas Ersa hasn't built yet, drawn from earlier agent-harness work and from prior art such as VS Code's remote server. Each was checked against Ersa's briefs, recorded fixtures and code on 2026-11-03. Lessons that applied at once went to `docs/briefs/INBOX.md` instead.

Treat a lesson as evidence to recheck, not a requirement. Version numbers say where a behavior was seen; when a lesson shapes a brief, confirm it on Ersa's tested range and record the fact in the brief or a fixture.

Read the file for the area you're starting:

- `claude-code.md`: when changing how Ersa reads Claude Code's stream or session record: usage fields, models and efforts, listing and resuming, unfinished work.
- `copilot.md`: when building the Copilot adapter.
- `remote-hosts.md`: when building remote sessions.
- `workspaces.md`: when changing how workspaces are created, removed or reclaimed.
- `packs.md`: when building packs.
- `process.md`: when changing the orchestration procedure or the implementing session's test routine.

One lesson cuts across every area: make bounded time a rule. Every spawned process has a stop deadline and an escalation path, every round trip a timeout, every reconnect a backoff ceiling. Ersa has one-off deadlines, such as FLOW-CLOSE-AND-QUIT-R06 and `core.hello`'s, but no general rule in ARCH-CORE.
