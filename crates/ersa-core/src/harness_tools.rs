//! Harness tools server: an MCP server exposed to every session's backend
//! (`PROTO-HARNESS-TOOLS`). From milestone 0 it serves the permission-prompt tool
//! that Claude Code is pointed at (ADR 0012). Later it adds cross-session
//! messaging, session metadata, user-defined tools, and the interface-driving
//! tools. No harness tool can ever operate a permission control.
