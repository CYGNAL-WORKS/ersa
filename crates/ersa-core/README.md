# ersa-core

The library that is Ersa's Core: sessions, raw logs and projections, the adapter registry, transports, workspaces and VCS, packs, modes, the harness tools server, and persistence. The `ersa-core` binary in `ersa-cli` is a thin wrapper around it, and the same binary runs on remote hosts. It must never depend on Tauri or anything that needs a window or display, so it builds and tests on a headless machine. Code that only works on one operating system belongs behind a narrow `platform` module, and `tests/boundaries.rs` checks both rules.
