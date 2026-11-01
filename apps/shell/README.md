# apps/shell

The Shell (Tauri 2, `src-tauri/`) and the Interface (React 19 and TypeScript, `src/`) of ADR 0008. The Shell launches or attaches to Core and owns the pipe; the Interface reaches Core only through the Shell's two bridge commands, with types generated from `protocol/schema/` (`src-tauri/src/lib.rs` documents the bridge).

Run the window during development, from the repository root:

1. `cargo build -p ersa-cli`, so the Shell finds `target/debug/ersa-core` (or set `ERSA_CORE_BIN`).
2. `corepack enable pnpm` (corepack 0.36 or later; it fetches the pnpm `package.json` pins), `pnpm install`, then in `apps/shell`: `pnpm tauri dev`.

In `apps/shell`:

- `pnpm gen` regenerates `src/protocol/*.gen.ts` from the schemas and `src/theme/tokens.gen.css` from `docs/design/DESIGN.md`; CI fails when they are stale.
- `pnpm typecheck`, `pnpm test` (unit tests, including the check that interface code uses only tokens).
- `pnpm dev`, then open `/gallery.html`: every component in every state, light and dark. Add a component there in the change that builds it.
- `pnpm test:screenshots` photographs the gallery. Baselines are Linux renders from the Playwright container; `pnpm test:screenshots:update` (Docker) rewrites them, or take them from the `screenshots` artifact of a failed CI run.
- The Shell's Rust side is its own Cargo workspace: `cargo test --manifest-path src-tauri/Cargo.toml` (needs `ersa-core` built, as above). Each test's Core serves an endpoint of its own, so a Core of yours can keep running.
