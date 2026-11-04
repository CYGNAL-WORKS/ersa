# Claude Code: lessons for later

Ersa's tested range was 2.1.316 when these were checked. Most were seen on older versions; each lesson says what Ersa's own fixtures show.

## Stream and usage

- **Interrupt receipt.** After an interrupt the live stream carries a `user` text frame `[Request interrupted by user]`, even without `--replay-user-messages` (seen on 2.1.214). PROTO-CLAUDE-CODE-STREAM's `user` section lists only echoes and tool results. The record-side marker is already in PROTO-AGENT-EVENTS; the record's synthetic `No response requested.` message is not.
- **Context window.** No brief names the source of `usage.updated.context_window`. The adapter reads `result.modelUsage.contextWindow` (1,000,000 in the 2.1.316 fixtures), and `initialize`'s `models[]` has no window field. Older versions reported the base window in `modelUsage` for `[1m]` model ids, which the 2.1.316 catalog no longer lists; `get_context_usage.maxTokens` reflected the real tier.
- **Rate-limit units.** In `rate_limit_event`, `resetsAt` is epoch seconds (the adapter converts it), `utilization` is a fraction that can exceed 1 and that Ersa doesn't carry, and `status: "allowed"` is routine, about once per process in the fixtures. `get_usage` uses 0 to 100 and ISO timestamps instead. PROTO-AGENT-EVENTS states no units.

## Models

- **Detected choices.** PROTO-CORE-API says `backend.list`'s models and efforts are "not detected on the host", which leaves FLOW-NEW-SESSION's open question in place. `initialize`'s `models[]` carries per-model `supportedEffortLevels`, filtered by provider and policy. In the 2.1.316 fixture `claude-opus-4-6` lacks `xhigh` and `claude-haiku-4-5` has none, though the four aliases Ersa lists take all five. Detecting means asking the CLI before any session exists; weigh that cost.
- **Effective model.** A session can run on a model other than the one named. Ersa passes no `--fallback-model`, but the user's `fallbackModel` setting (2.1.166) loads through `--setting-sources`. The binary defines a `system/model_fallback` message that hasn't been seen live.

## Listing and resuming

- **Shared record directories.** PROTO-CLAUDE-CODE-STREAM-R16 lists a directory's sessions without checking each record's `cwd`, but distinct paths share a directory. Ersa's own encoding maps `a\b`, `a b`, and `a-b` to one name, and the CLI once resumed a session from a directory that differed only in `_`, `-`, or `.` (CLI release notes, 2.1.239). Record entries carry `cwd` (2.1.316 record fixtures), so listing can filter on it.
- **Spawn directory.** Core spawns and resumes in the stored workspace path, but no brief requires it. The CLI's reported cwd is normalized (a `\\?\` path on Windows), and a fork spawned there fails with `ENOENT`.
- **Missing directory.** For FLOW-RESUME-SESSION's open question: the CLI's interactive `/resume` once ran in the current directory (2.1.239), and `-p --resume` resumed without the worktree after its git metadata was lost (2.1.260).

## Unfinished work

- Turn-level interruption is specified, but a `tool.requested` with no `tool.result` after a stop or reopen, and the state of subagents, are not. A process-tree stop writes no result; since 2.1.265 the CLI's resume marks such a tool call interrupted, which no Ersa fixture records.
- Neither Claude Code nor Copilot can restore a native subagent after resume, so mark unresolved children interrupted rather than running. Bears on PROTO-AGENT-EVENTS' open question on subagent trees.
