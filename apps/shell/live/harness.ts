// The live-check harness (docs/agents/live-checks.md): each live check drives
// the real Ersa window, attached to a Core of its own, with Playwright.
//
// Per test it starts `ersa-core serve` on a fresh endpoint label with a fresh
// data folder, then a debug `ersa` attached to that label (ERSA_ATTACH) with
// WebView2's remote debugging port open, and connects Playwright to the
// window over the Chrome DevTools Protocol. When the test ends, passed or
// failed, it ends both process trees and removes the temporary folder, and
// fails the test if anything is left.
//
// Native parts Playwright cannot reach are answered at the Shell's boundary:
// a system dialog by answering the Interface's request to the Shell's dialog
// command, and a file drop by delivering the window's drag-and-drop events as
// the Shell delivers them. Everything past that boundary is real.

import { type ChildProcess, execFile, execFileSync, spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { createWriteStream, existsSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { type Browser, type Locator, type Page, test as base, chromium, expect } from "@playwright/test";
import { binaries } from "./paths";

/** Which backend CLI Core starts for a session. */
export type Backend =
  /** The stand-in Core's own tests use: it answers the handshake and takes messages, without a model. */
  | "stand-in"
  /** The real Claude Code on this machine, with the owner's own login. */
  | "claude";

/** What a system dialog answers: a path, several paths, or `null` for Cancel. */
export type DialogAnswer = string | string[] | null;

/** A dialog the Interface opened, as the test answered it. */
export interface AnsweredDialog {
  /** `open` (files or a folder) or `save`. */
  command: string;
  /** The options the Interface passed, such as `{ directory: true }`. */
  options: unknown;
}

export interface Ersa {
  /** The Ersa window. */
  page: Page;
  /** The endpoint label this test's Core serves, which the rail's foot names. */
  label: string;
  /** This test's own temporary folder, removed when the test ends: put folders the test picks or drops here. */
  dir: string;
  /**
   * Answers the next system dialog the Interface opens; resolves once it has, with what the Interface asked for.
   * A dialog opened with no answer waiting fails in the Interface and fails the test.
   */
  answerDialog(answer: DialogAnswer): Promise<AnsweredDialog>;
  /** Drops `paths` on the window, over the center of `onto` or else the window's top left, as a drop from the file manager arrives. */
  drop(paths: string[], onto?: Locator): Promise<void>;
  /** Saves a screenshot of the window for the pull request, numbered in the order taken. */
  shot(name: string): Promise<void>;
}

/**
 * How long the start as a whole may take, Core to the loaded window; a first WebView2 start is slow. The start fails
 * by itself within it, and the fixture's own timeout is longer, so a slow start always reaches the cleanup below:
 * Playwright waits for a fixture's cleanup only once the fixture has handed over to the test.
 */
const START_LIMIT_MS = 60_000;

let counter = 0;

// What this worker started and has not yet ended. If the worker exits without a fixture's cleanup (Ctrl+C, a crash),
// this ends them as the process exits.
const started = new Set<ChildProcess>();
const folders = new Set<string>();
process.once("exit", () => {
  for (const child of started) {
    if (ended(child)) continue;
    try {
      execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    } catch {
      // Gone already.
    }
  }
  for (const dir of folders) rmSync(dir, { recursive: true, force: true });
});

export const test = base.extend<{ backend: Backend; ersa: Ersa }>({
  backend: ["stand-in", { option: true }],

  ersa: [
    async ({ backend }, use, testInfo) => {
      const bin = binaries();
      // A label is letters, digits, `-` and `_` (PROTO-CORE-API, Transport and framing); random, so a run never meets
      // the Core or window of another run, or of the owner.
      const label = `live-${process.pid}-${counter++}-${randomBytes(4).toString("hex")}`;
      const dir = mkdtempSync(path.join(tmpdir(), "ersa-live-"));
      folders.add(dir);
      const deadline = Date.now() + START_LIMIT_MS;
      const running: ChildProcess[] = [];
      let browser: Browser | undefined;
      let unanswered: string[] = [];
      try {
        // Logs go beside the screenshots, so they outlive the temporary folder for a failure to be read.
        const coreEnv: NodeJS.ProcessEnv = { ...process.env };
        if (backend === "stand-in") {
          coreEnv.ERSA_CLAUDE = bin.standIn;
          coreEnv.ERSA_STAND_IN_ANSWER = "1";
        } else {
          // Core prefers ERSA_CLAUDE to the `claude` on PATH; one inherited from the caller is not the real CLI.
          delete coreEnv.ERSA_CLAUDE;
        }
        running.push(
          start(bin.core, ["serve", "--endpoint", label, "--data-dir", path.join(dir, "data")], coreEnv, testInfo.outputPath("core.log")),
        );
        await waitFor(() => servesLabel(label), "Core to serve its endpoint", running, deadline);

        const port = await freePort();
        running.push(
          start(
            bin.shell,
            [],
            {
              ...process.env,
              ERSA_ATTACH: label,
              // WebView2 reads both: the remote debugging port Playwright attaches to, and a profile of the test's own,
              // so it never shares the browser process of the owner's Ersa or of another run.
              WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
              WEBVIEW2_USER_DATA_FOLDER: path.join(dir, "webview"),
            },
            testInfo.outputPath("shell.log"),
          ),
        );
        await waitFor(() => debuggerUp(port), "the window's debugging port", running, deadline);
        browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: remaining(deadline) });
        const page = await windowPage(browser, deadline);
        const dialogs = await answerDialogs(page);
        unanswered = dialogs.unanswered;
        let shots = 0;
        await use({
          page,
          label,
          dir,
          answerDialog: dialogs.answer,
          drop: (paths, onto) => drop(page, paths, onto),
          async shot(name) {
            shots += 1;
            const file = testInfo.outputPath(`${String(shots).padStart(2, "0")}-${name}.png`);
            await page.screenshot({ path: file });
            await testInfo.attach(name, { path: file, contentType: "image/png" });
          },
        });
      } finally {
        // Disconnects only; ending the processes below closes the window.
        await browser?.close().catch(() => {});
        for (const child of running.reverse()) await endTree(child);
        rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
        const left = [...running.filter((child) => !ended(child)).map((child) => `process ${child.pid}`), ...(existsSync(dir) ? [dir] : [])];
        if (left.length > 0) throw new Error(`the live check left behind ${left.join(", ")}`);
        folders.delete(dir);
      }
      if (unanswered.length > 0) throw new Error(`the Interface opened a dialog the test did not answer: ${unanswered.join("; ")}`);
    },
    // A budget of its own, apart from the test's, longer than the start may take plus the cleanup.
    { timeout: START_LIMIT_MS + 30_000 },
  ],
});

export { expect };

// The Interface reaches the Shell's commands through Tauri's IPC, which on Windows is a `fetch` POST to
// `http://ipc.localhost/<command>`; the dialog plugin's commands are `plugin:dialog|open` and `plugin:dialog|save`.
// WebView2 serves that address inside the webview, out of reach of Playwright's network routing, so the harness
// wraps the window's `fetch` and answers those two commands from the test: neither the Shell nor the system's dialog
// sees them. Every other request goes through untouched.
async function answerDialogs(page: Page) {
  const waiting: { answer: DialogAnswer; answered: (dialog: AnsweredDialog) => void }[] = [];
  const unanswered: string[] = [];
  await page.exposeBinding("__ersaLiveDialog", (_source, dialog: AnsweredDialog) => {
    const next = waiting.shift();
    if (!next) {
      unanswered.push(JSON.stringify(dialog));
      return { error: "live check: no answer was waiting for this dialog" };
    }
    next.answered(dialog);
    return { answer: next.answer };
  });
  const wrap = () => {
    const w = window as unknown as Record<string, unknown> & { fetch: typeof fetch };
    if (w.__ersaLiveFetch) return;
    w.__ersaLiveFetch = true;
    const original = w.fetch.bind(window);
    const dialog = /^http:\/\/ipc\.localhost\/plugin%3Adialog%7C(open|save)$/;
    w.fetch = async (input, init) => {
      const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
      const command = dialog.exec(url)?.[1];
      if (!command) return original(input, init);
      const { options } = JSON.parse(String(init?.body ?? "{}")) as { options?: unknown };
      const ask = w.__ersaLiveDialog as (d: AnsweredDialog) => Promise<{ answer?: DialogAnswer; error?: string }>;
      const reply = await ask({ command, options });
      // Answered as the Shell answers: `Tauri-Response: ok` with JSON, or an error as text. Never a rejected
      // fetch, which would make Tauri give up on this IPC for the rest of the page's life.
      return "error" in reply
        ? new Response(reply.error, { headers: { "Content-Type": "text/plain", "Tauri-Response": "error" } })
        : new Response(JSON.stringify(reply.answer), { headers: { "Content-Type": "application/json", "Tauri-Response": "ok" } });
    };
  };
  // For this page, and again for each reload of it.
  await page.addInitScript(wrap);
  await page.evaluate(wrap);
  return {
    unanswered,
    answer: (answer: DialogAnswer) => new Promise<AnsweredDialog>((answered) => waiting.push({ answer, answered })),
  };
}

// Where Tauri keeps the window's event listeners, and the function its Rust side calls to deliver an event to them
// (tauri's src/event/listener.rs). A Tauri that renames them fails the drop and says so.
const LISTENERS = "__internal_unstable_listeners_object_id__";
const DELIVER = "__internal_unstable_listeners_function_id__";

async function drop(page: Page, paths: string[], onto?: Locator): Promise<void> {
  // Tauri gives positions in physical pixels, from the top left of the window's content.
  let position = { x: 0, y: 0 };
  if (onto) {
    const box = await onto.boundingBox();
    if (!box) throw new Error("drop: the target is not on screen");
    const scale = await page.evaluate(() => window.devicePixelRatio);
    position = { x: Math.round((box.x + box.width / 2) * scale), y: Math.round((box.y + box.height / 2) * scale) };
  }
  // A drop from the file manager arrives as these events, in this order (Tauri's DragDropEvent).
  const events: [string, unknown][] = [
    ["tauri://drag-enter", { paths, position }],
    ["tauri://drag-over", { position }],
    ["tauri://drag-drop", { paths, position }],
  ];
  for (const [event, payload] of events) {
    await page.evaluate(
      ({ event, payload, LISTENERS, DELIVER }) => {
        const w = window as unknown as Record<string, unknown>;
        const deliver = w[DELIVER];
        if (typeof deliver !== "function") {
          throw new Error(`drop: Tauri's event delivery (${DELIVER}) is not on the window; the live-check harness needs updating`);
        }
        // Tauri defines each listener as a non-enumerable property named by its id.
        const listeners = (w[LISTENERS] as Record<string, object> | undefined)?.[event] ?? {};
        deliver({ event, payload }, Object.getOwnPropertyNames(listeners).map(Number));
      },
      { event, payload, LISTENERS, DELIVER },
    );
  }
}

/** Starts `program`, its output appended to `log`. */
function start(program: string, args: string[], env: NodeJS.ProcessEnv, log: string): ChildProcess {
  const child = spawn(program, args, { env, stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
  started.add(child);
  child.once("exit", () => started.delete(child));
  const out = createWriteStream(log, { flags: "a" });
  // A program that cannot start: said in its log, and `waitFor` sees it has no process.
  child.once("error", (e) => out.write(`live check: cannot start ${program}: ${e.message}\n`));
  child.stdout?.pipe(out);
  child.stderr?.pipe(out);
  return child;
}

/** Ends the process and everything it started (the window's WebView2 processes, Core's backend CLIs). */
async function endTree(child: ChildProcess) {
  if (ended(child)) return;
  // Fails when the process is already gone; `ended` below says.
  await promisify(execFile)("taskkill", ["/PID", String(child.pid), "/T", "/F"]).catch(() => {});
  // taskkill returns before the process is gone; Node marks it ended once it is.
  const until = Date.now() + 10_000;
  while (!ended(child) && Date.now() < until) await sleep(100);
}

/**
 * Whether the process has ended, or never started, as Node saw it. Never by its process id alone: Windows gives an
 * ended process's id to the next process, and ending that would end a stranger's.
 */
function ended(child: ChildProcess): boolean {
  return child.pid === undefined || child.exitCode !== null || child.signalCode !== null;
}

/** Whether Core serves `label`: its pipe is `ersa-core-<user SID>-<label>` (PROTO-CORE-API, Transport and framing). */
function servesLabel(label: string): boolean {
  const pipe = new RegExp(`^ersa-core-S-[0-9-]+-${label}$`);
  return readdirSync("\\\\.\\pipe\\").some((name) => pipe.test(name));
}

async function debuggerUp(port: number): Promise<boolean> {
  try {
    return (await fetch(`http://127.0.0.1:${port}/json/version`)).ok;
  } catch {
    return false;
  }
}

/** Waits for `ready`, failing early if one of the processes exits. */
async function waitFor(ready: () => boolean | Promise<boolean>, what: string, running: ChildProcess[], deadline: number) {
  while (Date.now() < deadline) {
    if (await ready()) return;
    const exited = running.find(ended);
    if (exited) throw new Error(`waiting for ${what}: ${path.basename(exited.spawnfile)} exited with ${exited.exitCode}; its log is in the test's results`);
    await sleep(100);
  }
  throw new Error(`${what} did not come up within ${START_LIMIT_MS / 1000} s of the start`);
}

/** What is left of the start's time; at least a moment, so a call fails with its own error. */
function remaining(deadline: number): number {
  return Math.max(deadline - Date.now(), 1);
}

async function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close(() => (typeof address === "object" && address ? resolve(address.port) : reject(new Error("no free port"))));
    });
  });
}

/** The window's page, once the Interface has loaded in it. */
async function windowPage(browser: Browser, deadline: number): Promise<Page> {
  while (Date.now() < deadline) {
    const page = browser.contexts().flatMap((context) => context.pages())[0];
    if (page) {
      // The window starts on a blank page and then navigates to the Interface embedded in the Shell.
      await page.waitForURL(/^http:\/\/tauri\.localhost\//, { waitUntil: "load", timeout: remaining(deadline) });
      return page;
    }
    await sleep(100);
  }
  throw new Error("the window opened no page");
}

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
