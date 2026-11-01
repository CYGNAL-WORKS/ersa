# ersa-cli

The crate that builds the `ersa-core` executable, the Core process that the Shell launches or attaches to (ADR 0011). The same binary runs on remote hosts. It parses arguments and hands off to the `ersa-core` library; for now `serve` only prints "not implemented". It must stay thin, with no domain logic of its own, and must never depend on Tauri or anything that needs a window or display.
