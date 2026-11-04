# Process: lessons for later

The `tdd`, `writing-for-agents` and `diagnosing-bugs` skills are vendored, so these land in Ersa's own skills or `docs/agents/`.

- **Machine load.** `implement-ticket` step 4.4 doesn't say how to read a failure caused by load, though worktree sessions and the Windows runner share one machine. A timeout outside the diff that fails only on Windows is probably load, so compare with the hosted Linux job; when a test fails only in the full run, cut its setup cost rather than raise the timeout.
- **Real-CLI test schedule.** `crates/ersa-adapter-claude/tests/live.rs` runs only by hand, and no step names when. ADR 0014 makes a CLI change a fixture re-recording, and PROTO-CLAUDE-CODE-STREAM-R10 warns above the tested range, but nothing asks for a live run on a ticket that changes how the adapter launches the CLI, or at a milestone's close. An unattended schedule would spend the owner's login, so an owner check fits. A contract suite that never runs reads as coverage while wire drift regresses.
- **A stopped session.** `docs/agents/orchestration.md` doesn't say what to do when an implementing session stops with its work unfinished: inspect its worktree diff, then resume it or take over.
- **Reading the pull request back.** The orchestrator's merge step checks CI and the merge base, but not that the pull request targets `main` and says `Fixes #n`; a pull request stacked on another branch merges into the wrong place.
- **Bounding an unattended run.** An unattended orchestration run has no stop conditions in the repository, and nothing says it handles only pull requests from sessions it started; otherwise new pull requests keep arriving and the run never ends.
