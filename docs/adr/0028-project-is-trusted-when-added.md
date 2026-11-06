# 0028: A project is trusted when added, after a question that reads like the CLI's; only a trusted project's own configuration loads

- Status: accepted
- Date: 2026-11-06
- Supersedes: 0018
- Related briefs: FLOW-NEW-SESSION, PROTO-CORE-API, PROTO-CLAUDE-CODE-STREAM, LAY-SHELL

## Context

ADR 0018 has Ersa ask whether the user trusts a project when they add it, and remember the answer. Its question offered three answers: add trusted, add not trusted, and cancel. The Claude Code CLI asks the same thing in a terminal before it loads a folder's configuration, and that prompt offers two: exit, selected by default, or trust the folder. Users of Ersa already know the CLI's prompt. The owner decided Ersa's question should read and behave like it.

## Options considered

1. **Keep ADR 0018's three answers**: a project can be added untrusted in one step, but the question looks unlike the prompt users know, and its extra answer invites adding a project without deciding.
2. **Two answers, as the CLI has**: cancel first and focused, or trust. A project the user does not trust is not added; one they stop trusting is marked untrusted later.

## Decision

Option 2, and ADR 0018's decision otherwise stands, restated here so this record is whole: Ersa asks once per project, when the user adds it, and remembers; the trust covers every workspace of the project and lasts until the user withdraws it, which they can do later from the project's heading. A trusted project's sessions load the project's and the local settings and the project's MCP configuration (ADR 0026, restating 0017). An untrusted project's sessions load only the user's own settings and none of the repository's configuration. Where a backend keeps its own trust record and offers a safe way to set it, Ersa marks a trusted project's workspaces trusted there too, so resuming a session in the backend's own tools does not ask again.

For the question: it shows the folder's path, asks whether this is a project the user created or trusts and to review it first if not, and says that agents will be able to read, edit and run files there, keeping the CLI's meaning and shape in Ersa's own words (CONTENT.md). Its answers are Cancel, first and focused, and Trust. Adding a project is trusting it.

## Consequences

- A repository's configuration still never runs before the user has said they trust it.
- Every project added from now on starts trusted, and becomes untrusted only when the user withdraws its trust. Projects added untrusted before this decision stay untrusted until the user trusts them.
- The CLI's prompt links to a security guide. Ersa's question has none until Ersa has a guide of its own to link (FLOW-NEW-SESSION, Open questions).
- The Core API keeps `trusted` on `project.add`, so Core holds no policy about the question's answers; the Interface always sends `true`.
- For Claude Code, the trust Ersa records reaches the CLI through `initialize`'s `workspaceTrust` (PROTO-CLAUDE-CODE-STREAM), which the CLI keys to the repository's git root. So a project added at a subfolder trusts the whole repository in the CLI, and withdrawing trust in Ersa leaves the CLI's record, so terminal sessions there stay trusted. The owner chose to match the CLI's prompt, which says nothing of this, so the question doesn't surface it.
