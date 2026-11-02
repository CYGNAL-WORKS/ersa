# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-02 ADR 0018: the trust Ersa attests is the CLI's to keep: revoking a project's trust in Ersa stops further attestations but leaves the CLI's record, so the repository stays trusted for terminal sessions; and the CLI keys trust by git root, so a project added at a subfolder of a repository grants trust to the whole repository. Built as the issue directs; whether either needs a user-facing note or a different rule is the owner's call.
- [ ] 2026-11-02 PROTO-CLAUDE-CODE-STREAM: #46 built: for a trusted project's session `initialize` carries `workspaceTrust { accepted: true, directory: <project root> }` and an untrusted one's never does; a `workspace_trust_recorded` other than `true` is a warning in Core's tracing log, not a failed start. Proposed requirement: "When the session's project is trusted, the adapter shall send `workspaceTrust` with `accepted: true` and the project's root in `initialize`; otherwise it shall send no `workspaceTrust`." Its tests cite no ID until it has one. Fixture 2.1.316/03-workspace-trust records the exchange; the Invocation paragraph and the open question on the CLI's trust record can be updated (the CLI keys trust by git root, so a linked worktree's sessions share the project's record).
- [ ] 2026-11-02 PROTO-CLAUDE-CODE-STREAM: open: whether `permissions.allow` applies already in the session that first attests trust (the CLI logs "re-applying project settings after recording workspace trust", but its stderr warning is printed before `initialize` is read). Checking needs a tool call, so a model call; the opt-in live test checks only that the next session loads the rule without the warning.
- [ ] 2026-11-02 none: SessionConfig gained `project_path` (the project's root, the folder the trust question was about) so the adapter can name it; ARCH-CORE or PROTO-CORE-API may want to say that Core passes the project's root, not only its trust, to a backend.

## Resolved recently (optional, trimmed in planning passes)
