# 0035: The daily build is an unsigned installer built locally from main, on its own endpoint and data folder

- Status: accepted
- Date: 2026-11-13
- Related briefs: ARCH-CORE, LAY-SHELL, FLOW-CLOSE-AND-QUIT

## Context

M1's dogfood week needs an Ersa the owner uses every day instead of Claude Desktop. Today Ersa runs only from a development build, which uses the user's default Core endpoint and data folder, so a development window, a crash in a half-built branch, or a database migration on a branch would reach the sessions the owner works in. Live checks already run their own Core on a random label with a temporary data folder. There is no installer: the Tauri bundle is off, and the README says nothing runs. Signing and automatic updates cost a certificate and an update service the project doesn't need yet.

## Options considered

1. **Run the development build daily.** Nothing to build, but every branch the owner runs touches the same Core and data, and Core stops whenever a development build is rebuilt.
2. **Signed releases with automatic updates.** What users will get eventually, but needs a certificate, a release pipeline and an update feed before the dogfood week.
3. **An unsigned installer built locally from `main` on demand, on its own endpoint label and data folder** (the owner's decision, 2026-11-13).

## Decision

Option 3.

- **The daily build** is a release build of `main` with the bundle on, producing a Windows installer that installs the window and Core side by side for the current user. It is unsigned; Windows' SmartScreen warning on first run is expected.
- **Its own endpoint and data folder.** A release build serves and attaches to the endpoint label `daily` and keeps its data in the `daily` folder inside Ersa's application data folder (`%LOCALAPPDATA%\Ersa\daily` on Windows), both fixed when it is built; nothing in the environment changes them. It is the daily build's own Core, not one it attaches to: the window launches it there (`serve` with that label and data folder, its log in that folder), quitting shuts it down as quitting always does, and the rail's foot stays empty. The window's one-per-user claim is the daily build's own too, so a daily window and a development window run side by side, each with one Shell. The installer puts `ersa-core` beside the window, where a release build looks for it. A development build keeps the user's default endpoint and data folder and refuses to attach to the label `daily`, and a live check keeps its own random label and temporary folder, so neither can reach the daily build's Core or data.
- **One script builds it.** A script in `scripts/` fetches `origin/main`, builds it in a clean checkout of its own, so the owner's working trees are untouched, and prints the installer's path; given a flag, it also runs the installer. It refuses when the checkout isn't at `origin/main`. Installing over an earlier daily build keeps its data folder.
- **Signing and automatic updates wait** until Ersa is offered to someone other than the owner, and that is a new decision.
- The README says how to build and install the daily build, in place of "Nothing runs yet".

## Consequences

The owner's daily sessions survive any branch they run and any live check. The daily build is as fresh as the last time the owner ran the script: updating is running it again, and the installer's own uninstall removes it. Projects, workspaces and arrangements made in a development build don't carry over to the daily build, and the reverse; a project is added again in each. When Ersa ships to others, the release build's label and folder are reconsidered, since a user's only build should be the default.
