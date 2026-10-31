# 0011: Core runs as its own process from the first milestone

- Status: accepted
- Date: 2026-10-31
- Related briefs: ARCH-CORE, PROTO-CORE-API

## Context

The first architecture draft kept Core as a library inside the shell process "until there is a reason otherwise." Review surfaced two reasons that arrive early: sessions should survive the window closing and allow the interface to detach and reattach, and remote sessions are simplest when the same Core binary runs on the remote host with the interface connecting over a tunnel. Retrofitting a process boundary after the shell and Core are intertwined costs weeks.

## Options considered

1. **Library in the shell process, daemon later**: least code now; the boundary is never designed and later becomes a rewrite.
2. **Core as a separate process from day one**, with the shell connecting over a local socket using a typed protocol: the boundary is designed once and reused for remote.

## Decision

Option 2. Core is a library crate wrapped in a thin binary. The shell launches it (or attaches to a running one) and speaks two protocols over a local socket: a request/response API (`PROTO-CORE-API`) and a per-session event stream (`PROTO-AGENT-EVENTS`). A remote host runs the same binary; the shell reaches it through an SSH tunnel.

## Consequences

Local and remote are one code path. Closing the window does not stop sessions; quitting does, unless the user asks otherwise. The interface's only dependency on Core is the two protocols, which makes the boundary testable and makes a replay server (a fake Core fed by recorded fixtures) a natural development tool. Costs: process lifecycle management on Windows (job objects so no orphans), a socket protocol to version, and slightly more M0 work.
