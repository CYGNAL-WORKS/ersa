# Process: lessons for later

The `tdd`, `writing-for-agents`, and `diagnosing-bugs` skills are vendored, so these land in Ersa's own skills or `docs/agents/`.

- **Machine load.** `implement-ticket` step 4.4 doesn't say how to read a failure caused by load, though worktree sessions share one machine with each other and with Docker. A timeout outside the diff that fails only locally is probably load, so compare with the hosted CI run; when a test fails only in the full run, cut its setup cost rather than raise the timeout.
- **Real-CLI test schedule.** `crates/ersa-adapter-claude/tests/live.rs` runs only by hand, and no step names when. ADR 0014 makes a CLI change a fixture re-recording, and PROTO-CLAUDE-CODE-STREAM-R10 warns above the tested range, but nothing asks for a live run on a ticket that changes how the adapter launches the CLI, or at a milestone's close. An unattended schedule would spend the owner's login, so an owner check fits. A contract suite that never runs reads as coverage while wire drift regresses.
- **A stopped session.** `docs/agents/orchestration.md` doesn't say what to do when an implementing session stops with its work unfinished: inspect its worktree diff, then resume it or take over.
- **Reading the pull request back.** The orchestrator's merge step checks CI and the merge base, but not that the pull request targets `main` and says `Fixes #n`; a pull request stacked on another branch merges into the wrong place.
- **Bounding an unattended run.** An unattended run needs a fixed set of work, a done condition, and a budget. `docs/agents/orchestration.md` has no such rule for an unattended orchestration run; without one, new pull requests keep arriving and the run never ends.
