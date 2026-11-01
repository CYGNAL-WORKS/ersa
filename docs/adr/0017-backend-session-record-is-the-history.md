# 0017: The backend's own session record is a session's history; Ersa sessions are ordinary backend sessions

- Status: accepted
- Date: 2026-11-01
- Supersedes: 0010
- Related briefs: ARCH-CORE, PROTO-AGENT-EVENTS, PROTO-CLAUDE-CODE-STREAM, PROTO-COPILOT-SDK, FLOW-RESUME-SESSION

## Context

ADR 0010 made Ersa's verbatim copy of each backend's output stream the persisted truth. The owner wants to move one session freely between Ersa and the backend's own tools (the Claude Code terminal and VS Code extension, the Copilot CLI) and to resume, in Ersa, sessions started elsewhere. A private copy cannot see turns taken outside Ersa, and each backend already keeps the record that resume uses.

What we found (2026-11-01, Claude Code 2.1.316, Copilot SDK 1.0.x):

- **Claude Code** keeps each session at `~/.claude/projects/<encoded cwd>/<session id>.jsonl`. Every `assistant` and `user` message Ersa receives on the stream appears in that file with the same `uuid` and identical `message` content. The file also records each tool call's permission decision. It omits text deltas, permission prompts, `result` (turn end, cost) and rate limits. Its entry wrapper is documented as internal and changing between versions. The supported readers (`listSessions`, `getSessionMessages`) exist only in the TypeScript and Python Agent SDKs. `--resume <id>` keeps the id and appends.
- **Copilot** keeps sessions under `~/.copilot/session-state/<id>/`. The official SDK, Rust included, lists sessions (`list_sessions`), resumes them, and returns their history (`get_events`) as the same event type as the live stream, minus ephemeral events such as deltas.
- The VS Code Claude Code extension launches the CLI through the Agent SDK with the user's settings loaded (`--setting-sources user,project,local`), sets its own entrypoint (`CLAUDE_CODE_ENTRYPOINT=claude-vscode`), lists sessions by reading the session files, and resumes by id. The CLI hides sessions whose entrypoint is `sdk-cli`, `sdk-ts` or `sdk-py` from the terminal picker, `--continue` and VS Code's list.
- Retention belongs to each backend: Claude Code deletes sessions after `cleanupPeriodDays` (30 by default).

## Options considered

1. **Keep 0010** (Ersa's own copy is the truth): one format per backend, independent of vendor retention. Cannot show work done outside Ersa, and disagrees with the record resume actually uses.
2. **The backend's record is the history; Ersa keeps only what lets it find the session again, plus an app log**: one truth, shared with the backend's own tools. Depends on a record format that is internal for Claude Code, and on vendor retention.
3. **Hybrid** (backend's record plus Ersa's own per-session facts): option 2 with a second store to stitch in. Nothing found needs it: permission decisions are in Claude Code's record, and Core's operational facts belong in an app log.

## Decision

Option 2.

- A session's history is the backend's own session record, read through the adapter. A live session's events come from the backend's live stream; details only the live stream carries (text as it arrives, permission prompts, turn ends, cost, rate limits) are shown and not stored.
- Per session, Ersa stores only what it needs to find and present it: host, project, workspace, backend, and the backend's session id. Core's operational records (process started, exited, failed to start, Core restarted) go to an app log, not into a session.
- Ersa sessions are ordinary backend sessions, launched the way the vendor's own IDE integration launches them. For Claude Code that means the user's settings load (user, project and local, with the user's MCP configuration), `CLAUDE_CODE_ENTRYPOINT` is set to `ersa` so the sessions appear in the backend's own session lists, and Claude variables inherited from whatever started Ersa are cleared.
- Ersa lists and resumes any session in the backend's record for a project, including ones started elsewhere, and its own sessions can be resumed elsewhere by id.
- Retention is the backend's: when the backend deletes a session, Ersa no longer shows it.

## Consequences

- The translation to Ersa events stays; it gains a second input. For Claude Code the stored messages equal the streamed ones, so one translation of messages serves both, and the record's wrapper is pinned the way the stream is: fixtures recorded per CLI version, and a warning above the newest tested version. For Copilot both inputs are the SDK's own event type.
- Ersa's raw log of each session's stream is retired as a store of truth; what M0 built becomes the app log or is removed.
- A session's view is the record plus the live stream since the session was opened, merged on the backend's message ids.
- Sessions behave the same in Ersa and in the terminal, including the user's hooks, instructions and always-allow rules. Ersa no longer controls a session's configuration beyond the flags it passes.
- `ersa` is not a documented entrypoint value. If a CLI release stops honouring it, Ersa's sessions hide from the backend's own lists again and resume by id still works; a fixture re-recording catches any other change.
- Ersa does not prevent a session being open in two places at once. It checks the backend's record of live sessions before resuming and says so; otherwise both transcripts interleave in the backend's record.
- Revisit if a backend's record stops carrying what the transcript needs, or a supported reader with a stable format appears for Claude Code in a language Core can use.
