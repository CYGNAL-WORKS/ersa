# Live checks

A live check is a Playwright test that drives the real Ersa window on Windows. Write one when a ticket's **Done when** asks for a check in the running app, or when you need to see your change work beyond the unit tests and the gallery. Live checks run on your machine, never in CI.

## Run one

From `apps/shell`, on Windows, with `pnpm install` done:

- `pnpm live live/smoke.live.ts` runs one file; `pnpm live` runs them all.

Each run first builds what it drives from this checkout: the Interface (`pnpm build`), then Core, the stand-in backend, and a debug `ersa` with the Interface embedded, into `target/live-check`. The first build takes a few minutes; later runs take seconds.

Each test gets its own Core (`ersa-core serve` on a random endpoint label, with a data folder in a new temporary folder) and its own window attached to that label. Your own Ersa and Core keep running and are never touched. When the test ends, passed or failed, the harness ends both process trees, removes the temporary folder, and fails the test if anything is left. `live/cleanup.live.ts` checks this.

## Write one

Put it in `apps/shell/live/<name>.live.ts`, and import `test` and `expect` from `./harness`. The `ersa` fixture gives you:

- `page`: the window, a Playwright `Page`. Find elements by role and name, as the user sees them.
- `label`: the endpoint label, which the rail's foot shows as "Attached to <label>".
- `dir`: the test's temporary folder. Make the folders and files the test picks or drops here.
- `answerDialog(answer)`: answers the next system dialog the Interface opens, with a path, a list of paths, or `null` for Cancel. Call it before the step that opens the dialog, and await it afterward: it resolves with the command and the options the Interface asked for. A dialog that opens with no answer waiting fails the test.
- `drop(paths, onto?)`: drops files and folders on the window, over the center of the `onto` locator, as a drop from File Explorer arrives (drag-enter, drag-over, then drag-drop).
- `shot(name)`: saves a numbered screenshot. Take one after each step a reviewer should see.

Core starts the stand-in backend for sessions by default: it starts and takes messages without calling a model. For the real Claude Code, signed in as you on this machine, add `test.use({ backend: "claude" })` to the file.

`smoke.live.ts` is the smallest example, and `native.live.ts` shows a dialog and a drop.

## What is real

Everything from the Interface inward is real: the Interface, the Shell's bridge, Core, and the backend. Two native parts are answered by the test instead, at the Shell's boundary:

- A system dialog: the Interface's call to the Tauri dialog plugin's `open` or `save` command is answered from the test. The Shell and the system's dialog never see it.
- A file drop: the window receives Tauri's drag-and-drop events with the test's paths, exactly as the Shell delivers them. Unlike a real drop, Tauri doesn't add the dropped paths to its asset scope, so a preview that loads a dropped file through `convertFileSrc` won't load.

## Put the screenshots in the pull request

After the run, from the repository root:

- `node scripts/live-shots.mjs` pushes the screenshots under `apps/shell/live-results` to the branch `live-shots/<your branch>` and prints one Markdown image per screenshot. Paste them into the pull request's **Evidence**.

Run the live checks whose screenshots you want last, since the script publishes what the latest run left. When a check fails, read `core.log` and `shell.log` beside its screenshots in `apps/shell/live-results`, and the report in `apps/shell/live-report`.
