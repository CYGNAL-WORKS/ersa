# apps/shell

The Shell (Tauri 2, `src-tauri/`) and the Interface (React 19 and TypeScript, `src/`) of ADR 0008. The Shell launches or attaches to Core and owns the pipe; the Interface reaches Core only through the Shell's two bridge commands, with types generated from `protocol/schema/` (`src-tauri/src/lib.rs` documents the bridge).

Run the window during development, from the repository root:

1. `cargo build -p ersa-cli`, so the Shell finds `target/debug/ersa-core` (or set `ERSA_CORE_BIN`).
2. Install the pnpm that `packageManager` in `package.json` names: `npm install -g pnpm@12.15.0` (no admin rights needed). Then `pnpm install`, then in `apps/shell`: `pnpm tauri dev`.

   Corepack works too, as CI uses it, but only at 0.36 or later: older corepack, like the 0.33 that Node 22 bundles, can't start pnpm 12. On Windows, `corepack enable pnpm` writes into Node's install folder, so it needs an administrator shell.

To develop against the replay server instead of your own Core: `ersa-core replay --fixture <path>.jsonl` (it serves the label `replay`), then start the window with `ERSA_ATTACH=replay`. A debug build with `ERSA_ATTACH=<label>` attaches to that labelled endpoint and never launches Core there; a release build ignores it.

In `apps/shell`:

- `pnpm gen` regenerates `src/protocol/*.gen.ts` from the schemas, `src/theme/tokens.gen.css` from `docs/design/DESIGN.md`, and `src/catalog/en-US.gen.md` from the string catalog; CI fails when they're stale.
- `pnpm typecheck`, `pnpm test` (unit tests, including the checks that interface code uses only tokens and takes its copy from the string catalog).
- `pnpm dev`, then open `/gallery.html`: every component in every state, light and dark. Add a component there in the change that builds it.
- `pnpm test:screenshots` photographs the gallery. Baselines are Linux renders from the Playwright container. To rewrite them, push the branch, then run `node scripts/render-baselines.mjs` from the repository root: it starts the `Screenshot baselines` workflow on GitHub for the current branch, waits (up to half an hour; run it again and it waits on the same run), and replaces `e2e/__screenshots__/` with the artifact for you to review and commit. The artifact keeps the images of gallery tests that no longer exist; delete those yourself. It refuses a branch whose pushed head is not the local one, and GitHub can start the workflow only once it is on `main`. Agents use this; the owner can also render locally with `pnpm test:screenshots:update`, which needs Docker.
- `src/session/replays/*.json` are what the replay server sends for two fixtures, which the Interface's tests replay. The Shell's `tests/replay.rs` fails when they go stale; rewrite them with `ERSA_UPDATE_REPLAYS=1 cargo test -p ersa-shell --test replay`, which builds `ersa-core` first and stamps each capture with the commit it came from (`ersa_commit`), and review the diff.
- The Shell's Rust side is a member of the root Cargo workspace but not a default one, so a plain `cargo build` or `cargo test` leaves Tauri out: `cargo test -p ersa-shell` (needs `ersa-core` built, as above). Each test's Core serves an endpoint of its own, so a Core of yours can keep running. The quit tests run a stand-in backend CLI, `examples/stand_in_backend.rs`, which `cargo test -p ersa-shell` builds; running one test file alone needs `cargo build -p ersa-shell --examples` first.

Closing the window hides it to the tray icon; Quit is in the icon's menu, and stops Core. Launching Ersa again shows the window already running.
