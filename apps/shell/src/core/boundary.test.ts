// The Interface reaches Core only through the Shell's bridge, which carries
// Core API requests and notifications and nothing else. This holds that as a
// check: no Tauri API but the bridge's own two calls, and only in the one
// module that wraps them; no network or process access anywhere.

import { readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(import.meta.dirname, "..");
const BRIDGE = join("core", "tauri.ts");

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.tsx?$/.test(entry.name) && !entry.name.includes(".test.") ? [path] : [];
  });
}

const files = sourceFiles(SRC).map((path) => ({ path: relative(SRC, path), text: readFileSync(path, "utf8") }));

describe("the Interface's boundary", () => {
  // verifies: FLOW-NEW-SESSION-R03
  it("creates workspaces and sessions only in the start sequence, which runs on the first message", () => {
    const found = files
      .filter(({ path, text }) => path !== join("session", "start.ts") && !path.startsWith("protocol") && /["'](workspace|session)\.create["']/.test(text))
      .map(({ path }) => path);
    expect(found).toEqual([]);
  });

  // verifies: ARCH-CORE-R02
  it("uses Tauri only for the bridge's invoke and listen, the folder dialog and drag and drop, in one module", () => {
    const found: string[] = [];
    for (const { path, text } of files) {
      for (const [, module] of text.matchAll(/from\s+["'](@tauri-apps\/[^"']+)["']/g)) {
        // The dialog plugin reaches the system's folder dialog, and the webview its drag-and-drop events (FLOW-SEND-MESSAGE-R08),
        // never Core (FLOW-NEW-SESSION-R27).
        const allowed = path === BRIDGE && ["@tauri-apps/api/core", "@tauri-apps/api/event", "@tauri-apps/api/webview", "@tauri-apps/plugin-dialog"].includes(module ?? "");
        if (!allowed) found.push(`${path}: ${module}`);
      }
    }
    expect(found).toEqual([]);
    const bridge = files.find((f) => f.path === BRIDGE)?.text ?? "";
    expect([...bridge.matchAll(/invoke(?:<[^>]*>)?\(\s*"([^"]+)"/g)].map((m) => m[1]).sort()).toEqual([
      "core_attach",
      "core_request",
      "shell_hide",
      "shell_quit",
      "shell_quit_heard",
      "shell_show",
    ]);
  });

  // verifies: ARCH-CORE-R02
  it("opens no network connection of its own", () => {
    const found = files
      .filter(({ text }) => /\b(fetch|WebSocket|XMLHttpRequest|EventSource)\s*\(/.test(text) || /new\s+(WebSocket|EventSource)\b/.test(text))
      .map((f) => f.path);
    expect(found).toEqual([]);
  });
});
