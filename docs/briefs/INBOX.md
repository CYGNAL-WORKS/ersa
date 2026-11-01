# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-10-31 ARCH-CORE: `ersa-core serve` cannot be told which endpoint to serve, and on Windows nothing overrides the per-user pipe name, so the Shell's bridge test (which launches a real Core) uses the user's own pipe and fails if their Core is running; on Unix it uses `XDG_RUNTIME_DIR`. Proposal: a `--endpoint` option on `serve`. Meanwhile the test says to stop the running Core.
- [ ] 2026-10-31 none (DESIGN.md): there is no token for line width, so hairline borders in the interface are `1px`, the one raw value the interface's raw-value check allows. Proposal: a `line` width token.
- [ ] 2026-10-31 none (ADR 0015): Tauri depends on five MPL-2.0 crates (cssparser, cssparser-macros, dtoa-short, option-ext, selectors), which deny.toml does not allow. Meanwhile CI checks only bans and sources for the Shell's lockfile; allowing MPL-2.0 (file-level copyleft) in deny.toml is the owner's call.

## Resolved recently (optional, trimmed in planning passes)
