# ersa-cli

The crate that builds the `ersa-core` executable, the Core process that the Shell launches or attaches to (ADR 0011). The same binary runs on remote hosts. It parses arguments and hands off to the `ersa-core` library: `ersa-core serve [--data-dir DIR]` serves the Core API on the per-user named pipe until `core.shutdown`, and exits with a clear message if another Core already owns the pipe. It must stay thin, with no domain logic of its own, and must never depend on Tauri or anything that needs a window or display.
