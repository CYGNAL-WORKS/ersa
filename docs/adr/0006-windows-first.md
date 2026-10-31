# 0006: Windows first, core kept platform-neutral

- Status: accepted
- Date: 2026-10-31
- Related briefs: ARCH-CORE

## Context

The user develops on Windows 11 and will dogfood there. macOS is a plausible later target; Linux is undecided. Windows is the hardest platform for process and terminal work (ConPTY, path and line-ending differences, SSH agent behaviour).

## Options considered

1. **Windows only, use Windows APIs freely**: fastest. Rejected: a later macOS port becomes a rewrite of the process layer.
2. **All three platforms from day one**: rejected: triples the testing surface before there is anything to test.
3. **Windows first, platform-neutral core**: build and test on Windows; keep Windows-only APIs out of the core crates; isolate platform differences behind small modules.

## Decision

Option 3. CI runs on Windows. The core compiles on macOS and Linux even while untested there. Anything platform-specific lives behind a narrow interface with a Windows implementation and a stub or conditional compilation for the others.

## Consequences

Some Windows conveniences are forgone. Backends are driven through their structured protocols, never a PTY, so ConPTY is confined to the one place that needs it: the workbench's hosted terminal pane. Remote hosts are expected to be Linux, so the transport layer handles both path conventions from the start.
