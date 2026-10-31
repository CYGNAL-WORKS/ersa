---
id: PROTO-HARNESS-TOOLS
type: protocol
title: Harness tools server
status: draft
references: [PROTO-AGENT-EVENTS]
adrs: [0012]
---

# Harness tools server

## Intent

An MCP server that Ersa exposes to every session's backend. From milestone 0 it serves the permission-prompt tool that Claude Code is pointed at, so permission requests arrive as tool calls and are answered from the shelf. Later it adds: list sessions and their states, send a message to another session, read session metadata, user-defined tools the owner adds, and the interface-driving tools for show-me (focus a pane, highlight a control, explain). Each session receives a token identifying it; the token is advisory since all local sessions share the OS user. Interface-driving tools require visible per-invocation consent and can never operate permission controls. Remote hosts reach the server through a reverse tunnel on the host's SSH connection.

## Transport note (2026-10-31)

Claude Code's `initialize` control request accepts `sdkMcpServers`: the host declares in-process MCP servers and the CLI routes their traffic over the stdio control channel as `mcp_message` requests. If ADR 0014 is accepted, the harness tools server is served this way per session, with no socket, port, or token. The per-session identity question is answered by construction. Backends without an equivalent would use a stdio MCP server process instead.

## Open questions

- Whether every backend can reach an in-process server, or whether a stdio server process remains as the portable form.
- How users add their own tools: a directory of tool definitions, a script protocol, or both.
- Whether tool calls appear in the calling session's transcript.
- Rate limits and what happens when a session floods another with messages.

## Changes

- 2026-10-31: permission-prompt tool moved to milestone 0; user-defined tools added; permission controls forbidden; remote tunnel noted.
- 2026-10-31: created as draft.
