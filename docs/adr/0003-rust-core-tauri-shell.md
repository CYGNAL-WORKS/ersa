# 0003: Rust core with a Tauri shell and TypeScript UI

- Status: accepted
- Date: 2026-10-31
- Related briefs: ARCH-CORE

## Context

The predecessor was TypeScript on Electron. We want to avoid Electron's footprint, the user wants to learn Rust, and the interface must be highly polished and animated with a real design system. Native Rust and Go GUI toolkits cannot deliver that polish today. The GitHub Copilot SDK ships for Go, TypeScript, Python, and .NET but not Rust; under the hood it is JSON-RPC over stdio to the Copilot CLI binary.

## Options considered

1. **Go core + Wails**: gets the official Copilot SDK. Smaller desktop-shell ecosystem; Wails is less mature than Tauri.
2. **Rust core + Tauri 2**: the most mature non-Electron shell, a strong process and async story, no Copilot SDK so the adapter speaks the wire protocol directly. Satisfies the learning goal.
3. **Electron**: maximum UI maturity. Rejected on footprint and on principle.
4. **Rust core + native UI (egui, iced, Slint)**: rejected; cannot reach the required visual quality and animation.

## Decision

Option 2. The core (process management, transports, VCS, persistence, adapters, pack client, harness tools) is Rust. The shell is Tauri 2 on the OS webview. The interface is TypeScript running in that webview. The Copilot adapter implements the CLI's JSON-RPC protocol directly.

## Consequences

Two languages, with a typed boundary between them that must be kept honest: the agent event protocol (see ARCH-CORE). The codebase doubles as a Rust learning vehicle, so prefer plain idiomatic Rust over clever trait machinery and comment the why on ownership, async, and error-handling choices. The Copilot adapter is extra work compared with Go; it is bounded because we already have to document a vendor protocol for Claude Code.
