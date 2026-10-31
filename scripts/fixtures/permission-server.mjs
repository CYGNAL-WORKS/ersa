#!/usr/bin/env node
// Minimal stdio MCP server exposing one tool, `approve`, for recording how Claude Code
// uses --permission-prompt-tool. Every request is allowed and the full request is
// appended to the log file named by ERSA_PERMISSION_LOG so the fixture captures
// exactly what the CLI sends to a permission-prompt tool.
//
// Protocol: JSON-RPC 2.0, one message per line on stdin/stdout (MCP stdio transport).

import { appendFileSync } from "node:fs";

const LOG = process.env.ERSA_PERMISSION_LOG;
let buffer = "";

function log(entry) {
  if (!LOG) return;
  appendFileSync(LOG, JSON.stringify({ t: new Date().toISOString(), ...entry }) + "\n");
}

function reply(id, result) {
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id, result }) + "\n");
}

function handle(msg) {
  log({ direction: "in", msg });
  const { id, method, params } = msg;
  if (method === "initialize") {
    reply(id, {
      protocolVersion: params?.protocolVersion ?? "2025-06-18",
      capabilities: { tools: {} },
      serverInfo: { name: "ersa-permission-fixture", version: "0.0.1" },
    });
    return;
  }
  if (method === "tools/list") {
    reply(id, {
      tools: [
        {
          name: "approve",
          description: "Permission prompt handler used to record the protocol. Allows everything.",
          inputSchema: { type: "object", additionalProperties: true },
        },
      ],
    });
    return;
  }
  if (method === "tools/call") {
    const args = params?.arguments ?? {};
    const decision = { behavior: "allow", updatedInput: args.input ?? {} };
    reply(id, { content: [{ type: "text", text: JSON.stringify(decision) }] });
    return;
  }
  if (id !== undefined) {
    process.stdout.write(
      JSON.stringify({ jsonrpc: "2.0", id, error: { code: -32601, message: `method not found: ${method}` } }) + "\n"
    );
  }
}

process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  buffer += chunk;
  let i;
  while ((i = buffer.indexOf("\n")) !== -1) {
    const line = buffer.slice(0, i).trim();
    buffer = buffer.slice(i + 1);
    if (!line) continue;
    try {
      handle(JSON.parse(line));
    } catch (e) {
      log({ direction: "parse-error", line, error: String(e) });
    }
  }
});
process.stdin.on("end", () => process.exit(0));
