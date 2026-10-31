# 0009: Rust core, Tauri shell, TypeScript UI; Copilot via the official Rust SDK

- Status: accepted
- Date: 2026-10-31
- Supersedes: 0003
- Related briefs: ARCH-CORE, PROTO-COPILOT-SDK

## Context

ADR 0003 chose Rust and Tauri partly on the premise that no Rust SDK existed for GitHub Copilot, so the adapter would have to implement the CLI's JSON-RPC protocol by hand. An adversarial review found that premise false: `github/copilot-sdk` ships an official Rust crate (`github-copilot-sdk`, published on crates.io and updated the day of this record) alongside Go, TypeScript, Python, .NET, and Java. It spawns the Copilot CLI in server mode, performs the protocol-version handshake, handles process cleanup on Windows, and exposes permission handlers. Everything else in 0003 stands.

## Options considered

1. **Keep 0003 as written and hand-implement the protocol**: rejected; weeks of avoidable work and permanent chasing of the SDK's protocol version file.
2. **Use the official Rust SDK**: the adapter maps SDK types to Ersa events and nothing more.

## Decision

Option 2. The stack decision from 0003 is unchanged: Rust core, Tauri 2 shell, TypeScript interface. The Copilot adapter depends on the official Rust SDK. The protocol brief for Copilot becomes a mapping document, not a wire-protocol document. Rust is confirmed as the core language on its own merits (process management, async, single binary, mature non-Electron shell); the learning goal is a bonus, not a reason.

## Consequences

The Copilot adapter is now the cheaper of the two first adapters. The SDK's minimum CLI version becomes Ersa's. Decision records should cite verifiable facts; this one exists because 0003 did not.
