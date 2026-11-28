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
import { copyFileSync, createWriteStream, existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { connect, createServer } from "node:net";
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
  /**
   * Quits Ersa and opens it again, as a user closing it and starting it later would: ends the window and Core,
   * then starts Core on the same label and data folder and opens a new window. `page` is the new window's afterward.
   */
  restart(): Promise<void>;
  /** Closes the window as its close button does: the system's close message to the window. */
  closeWindow(): Promise<void>;
  /**
   * Opens the window again as a second launch of Ersa does: the Shell started once more finds the one running, which
   * shows its window with the function the tray icon's click calls, and the second exits.
   */
  openWindow(): Promise<void>;
  /** With `installedLater`: puts the stand-in where Core looks for the backend, as installing it while Ersa runs would. */
  installBackend(): void;
  /** Whether the window is on screen: the Shell's process has a visible top-level window titled Ersa. */
  windowShown(): Promise<boolean>;
  /** Whether the Shell's process and Core's are both still running. */
  running(): boolean;
}

/**
 * How long the start as a whole may take, Core to the loaded window; a first WebView2 start is slow. The start fails
 * by itself within it, and the fixture's own timeout is longer, so a slow start always reaches the cleanup below:
 * Playwright waits for a fixture's cleanup only once the fixture has handed over to the test.
 */
const START_LIMIT_MS = 60_000;

/**
 * How long a quit waits for what it ended to let go of its files. Under full load, the window's WebView2 browser
 * process took between 10 and 30 s; on an idle machine, well under one.
 */
const LET_GO_MS = 30_000;

let counter = 0;

// PowerShell, given the Shell's process as `$p`: True when that process owns a visible top-level window titled Ersa.
// Read through Windows' own window list, since a hidden window leaves the Interface's page reporting itself visible.
const WINDOW_SHOWN = `Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class ErsaWindows {
  public delegate bool Each(IntPtr window, IntPtr data);
  [DllImport("user32.dll")] static extern bool EnumWindows(Each each, IntPtr data);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr window, StringBuilder text, int size);
  public static bool Shown(int process, string title) {
    bool shown = false;
    EnumWindows((window, data) => {
      uint owner; GetWindowThreadProcessId(window, out owner);
      if (owner == process && IsWindowVisible(window)) {
        var text = new StringBuilder(256); GetWindowText(window, text, 256);
        if (text.ToString() == title) shown = true;
      }
      return true;
    }, IntPtr.Zero);
    return shown;
  }
}
'@; [ErsaWindows]::Shown($p.Id, "Ersa")`;

// What this worker started and has not yet ended. If the worker exits without a fixture's cleanup (Ctrl+C, a crash),
// this ends them as the process exits.
const started = new Set<ChildProcess>();
const folders = new Set<string>();
process.once("exit", () => {
  for (const child of started) {
    if (ended(child)) continue;
    try {
      execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore", windowsHide: true });
    } catch {
      // Gone already.
    }
  }
  for (const dir of folders) {
    try {
      rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    } catch {
      // Still held by a process going away; the other folders still go.
    }
  }
});

export const test = base.extend<{ backend: Backend; installedLater: boolean; signedOut: boolean; ersa: Ersa }>({
  backend: ["stand-in", { option: true }],
  /** With the stand-in: Core finds no backend until the test calls `installBackend()`. */
  installedLater: [false, { option: true }],
  /** With the stand-in: it reports that it is signed out, as a signed-out Claude Code does (FLOW-SIGN-IN). */
  signedOut: [false, { option: true }],

  ersa: [
    async ({ backend, installedLater, signedOut }, use, testInfo) => {
      const bin = binaries();
      // Which backend this test ran, beside its screenshots: scripts/live-shots.mjs publishes screenshots only from a
      // test whose record says `stand-in` (the file name is its BACKEND_RECORD). Written first, so no screenshot is
      // ever without one.
      writeFileSync(testInfo.outputPath("backend.txt"), backend);
      // A label is letters, digits, `-` and `_` (PROTO-CORE-API, Transport and framing); random, so a run never meets
      // the Core or window of another run, or of the owner.
      const label = `live-${process.pid}-${counter++}-${randomBytes(4).toString("hex")}`;
      const dir = mkdtempSync(path.join(tmpdir(), "ersa-live-"));
      folders.add(dir);
      const running: ChildProcess[] = [];
      // Processes of the Shell's and Core's trees that still held files or pipes once a quit had waited for them.
      const strays: Listed[] = [];
      let browser: Browser | undefined;
      const unanswered: string[] = [];
      let launches = 0;
      // The Shell's process, once launched, and the environment it was launched with.
      let shell: ChildProcess | undefined;
      let shellEnv: NodeJS.ProcessEnv = {};
      // Where Core looks for the stand-in when it is installed later: nothing is there until `installBackend()`.
      const later = path.join(dir, "later", path.basename(bin.standIn));
      // Runs a PowerShell expression about the Shell's process and returns what it printed.
      const aboutShell = async (expression: string) => {
        const { stdout } = await promisify(execFile)(
          "powershell",
          ["-NoProfile", "-NonInteractive", "-Command", `$p = Get-Process -Id ${shell?.pid}; ${expression}`],
          { windowsHide: true },
        );
        return stdout.trim();
      };
      // Starts Core, then the window attached to it, within the start's limit; returns the window.
      const launch = async () => {
        const deadline = Date.now() + START_LIMIT_MS;
        // Logs go beside the screenshots, so they outlive the temporary folder for a failure to be read; each
        // start after a restart has its own.
        launches += 1;
        const nth = launches;
        const log = (name: string) => testInfo.outputPath(nth === 1 ? `${name}.log` : `${name}-${nth}.log`);
        const coreEnv: NodeJS.ProcessEnv = { ...process.env };
        if (backend === "stand-in") {
          coreEnv.ERSA_CLAUDE = installedLater ? later : bin.standIn;
          coreEnv.ERSA_STAND_IN_ANSWER = "1";
          if (signedOut) coreEnv.ERSA_STAND_IN_SIGNED_OUT = "1";
          // Session records go to the test's own folder, never the owner's: the stand-in writes one with a session's
          // first message, as the CLI does, and Core reads them from there.
          coreEnv.CLAUDE_CONFIG_DIR = path.join(dir, "claude");
        } else {
          // Core prefers ERSA_CLAUDE to the `claude` on PATH; one inherited from the caller is not the real CLI.
          delete coreEnv.ERSA_CLAUDE;
        }
        running.push(start(bin.core, ["serve", "--endpoint", label, "--data-dir", path.join(dir, "data")], coreEnv, log("core")));
        await waitFor(() => corePipe(label) !== undefined, "Core to serve its endpoint", running, deadline);

        const port = await freePort();
        shellEnv = {
          ...process.env,
          ERSA_ATTACH: label,
          // The window opens without taking focus: Playwright drives it over CDP, which needs none.
          ERSA_BACKGROUND: "1",
          // WebView2 reads both: the remote debugging port Playwright attaches to, and a profile of the test's own,
          // so it never shares the browser process of the owner's Ersa or of another run.
          WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
          WEBVIEW2_USER_DATA_FOLDER: path.join(dir, "webview"),
        };
        shell = start(bin.shell, [], shellEnv, log("shell"));
        running.push(shell);
        await waitFor(() => debuggerUp(port), "the window's debugging port", running, deadline);
        browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: remaining(deadline) });
        const page = await windowPage(browser, deadline);
        const dialogs = await within(deadline, answerDialogs(page, unanswered), "the dialog answers to install");
        return { page, answerDialog: dialogs.answer };
      };
      // Ends the window, then Core, as quitting Ersa does.
      const quit = async () => {
        // Disconnects only; ending the processes below closes the window.
        await browser?.close().catch(() => {});
        browser = undefined;
        // Listed before anything ends, since a process whose parent has gone can no longer be found from it: the Shell
        // and Core, the window's WebView2 processes below the Shell, and the backend CLIs below Core. A listing that
        // fails still lets the trees be ended, and then fails the quit.
        let trees: Listed[] = [];
        let unlisted: unknown;
        try {
          trees = await processTrees(running.filter((child) => !ended(child)).map((child) => child.pid as number));
        } catch (e) {
          unlisted = e;
        }
        for (const child of [...running].reverse()) await endTree(child);
        // taskkill returns before what it ended has let go of its files and pipes. A restart's second window starts on
        // the same WebView2 user data folder, and its Core on the same pipe name, so both wait for that (#689).
        strays.push(...(await endListed(trees, LET_GO_MS)));
        if (unlisted !== undefined) throw unlisted;
        // Those ended are let go, so a restart's wait does not take one for its own start failing; any still
        // running stay for the check below.
        const left = running.filter((child) => !ended(child));
        running.splice(0, running.length, ...left);
      };
      try {
        let window = await launch();
        let shots = 0;
        const ersa: Ersa = {
          page: window.page,
          label,
          dir,
          answerDialog: (answer) => window.answerDialog(answer),
          drop: (paths, onto) => drop(ersa.page, paths, onto),
          async shot(name) {
            shots += 1;
            const file = testInfo.outputPath(`${String(shots).padStart(2, "0")}-${name}.png`);
            await ersa.page.screenshot({ path: file });
            await testInfo.attach(name, { path: file, contentType: "image/png" });
          },
          async closeWindow() {
            await aboutShell("[void]$p.CloseMainWindow()");
          },
          windowShown: async () => (await aboutShell(WINDOW_SHOWN)) === "True",
          async openWindow() {
            // The second launch asks the first to show its window, then exits on its own.
            await promisify(execFile)(bin.shell, [], { env: shellEnv, windowsHide: true, timeout: 30_000 });
          },
          installBackend() {
            mkdirSync(path.dirname(later), { recursive: true });
            copyFileSync(bin.standIn, later);
          },
          running: () => running.length > 0 && running.every((child) => !ended(child)),
          async restart() {
            await quit();
            if (strays.length > 0) {
              const which = strays.map(named).join(", ");
              throw new Error(`restart: ${which} still held files ${LET_GO_MS / 1000} s after Ersa quit, so the second window would not start`);
            }
            window = await launch();
            ersa.page = window.page;
          },
        };
        await use(ersa);
      } finally {
        await quit();
        rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
        const unended = running.filter((child) => !ended(child)).map((child) => child.pid);
        const left = [
          ...unended.map((pid) => `process ${pid}`),
          ...strays.filter((stray) => !unended.includes(stray.pid)).map(named),
          ...(existsSync(dir) ? [dir] : []),
        ];
        if (left.length > 0) throw new Error(`the live check left behind ${left.join(", ")}`);
        folders.delete(dir);
      }
      if (unanswered.length > 0) throw new Error(`the Interface opened a dialog the test did not answer: ${unanswered.join("; ")}`);
    },
    // A budget of its own, apart from the test's, shared by the start and the cleanup: the start's limit, then the
    // cleanup at its longest: the listing (15 s), two process trees of up to 10 s each, LET_GO_MS and its PowerShell's
    // margin (45 s), and the folder's retries, which back off by 250 ms more each time (about 50 s).
    { timeout: START_LIMIT_MS + 135_000 },
  ],
});

export { expect };

// The Interface reaches the Shell's commands through Tauri's IPC, which on Windows is a `fetch` POST to
// `http://ipc.localhost/<command>`; the dialog plugin's commands are `plugin:dialog|open` and `plugin:dialog|save`.
// WebView2 serves that address inside the webview, out of reach of Playwright's network routing, so the harness
// wraps the window's `fetch` and answers those two commands from the test: neither the Shell nor the system's dialog
// sees them. Every other request goes through untouched.
async function answerDialogs(page: Page, unanswered: string[]) {
  const waiting: { answer: DialogAnswer; answered: (dialog: AnsweredDialog) => void }[] = [];
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
  await promisify(execFile)("taskkill", ["/PID", String(child.pid), "/T", "/F"], { windowsHide: true }).catch(() => {});
  // taskkill returns before the process is gone; Node marks it ended once it is.
  const until = Date.now() + 10_000;
  while (!ended(child) && Date.now() < until) await sleep(100);
}

/**
 * A process from Windows' own process list. Its id and its start time together name it: Windows gives an ended
 * process's id to the next process, but never with the same start time.
 */
interface Listed {
  pid: number;
  name: string;
  /** When it started, as Win32_Process's CreationDate, in a form PowerShell reads back the same. */
  started: string;
}

function named(process: Listed): string {
  return `${process.name} (process ${process.pid})`;
}

/** Runs a PowerShell script that takes about `takes` ms at most, and parses the JSON array it prints. */
async function listed(script: string, takes = 0): Promise<Listed[]> {
  const { stdout } = await promisify(execFile)("powershell", ["-NoProfile", "-NonInteractive", "-Command", script], {
    windowsHide: true,
    maxBuffer: 16 * 1024 * 1024,
    // Room for PowerShell's own start on a busy machine, so one that hangs fails the quit rather than taking the
    // fixture's whole budget.
    timeout: takes + 15_000,
  });
  const text = stdout.trim();
  return text === "" ? [] : (JSON.parse(text) as Listed[]);
}

/**
 * The processes `roots` and everything below them, as Windows lists them now. A process counts as a child only if it
 * started no earlier than its parent, so one whose parent's id now belongs to a newer process is not taken.
 */
async function processTrees(roots: number[]): Promise<Listed[]> {
  if (roots.length === 0) return [];
  return listed(`
    $roots = @(${roots.join(",")})
    $all = @(Get-CimInstance Win32_Process)
    $tree = @{}
    foreach ($p in $all) { if ($roots -contains [int]$p.ProcessId) { $tree[[int]$p.ProcessId] = $p } }
    do {
      $grew = $false
      foreach ($p in $all) {
        $id = [int]$p.ProcessId; $parent = [int]$p.ParentProcessId
        if (-not $tree.ContainsKey($id) -and $tree.ContainsKey($parent) -and $p.CreationDate -ge $tree[$parent].CreationDate) {
          $tree[$id] = $p; $grew = $true
        }
      }
    } while ($grew)
    $listed = @($tree.Values | ForEach-Object {
      [pscustomobject]@{ pid = [int]$_.ProcessId; name = $_.Name; started = $_.CreationDate.ToString('o') }
    })
    ConvertTo-Json -Compress -InputObject $listed`);
}

/**
 * Ends each of `processes` still running, and waits up to `ms` for each to let go of what it held, in one PowerShell;
 * returns those that have not. `taskkill /T` reaches only what is still below a root when it runs, so a process whose
 * parent ended between the listing and the kill (a helper a git command leaves on purpose) is ended here.
 *
 * Ended is not enough: an ended process stays listed while Windows closes its handles, and on a busy machine the
 * window's WebView2 browser process took over 10 s to close its 1,161, the open files in the WebView2 user data
 * folder among them. A second window started then fails with "The requested resource is in use" (#689). A process
 * listed with no handles left holds nothing, so it counts as gone.
 */
async function endListed(processes: Listed[], ms: number): Promise<Listed[]> {
  if (processes.length === 0) return [];
  const wanted = JSON.stringify(processes).replace(/'/g, "''");
  return listed(
    `
    # Windows PowerShell's ConvertFrom-Json gives an array as one object; piping it on spreads it into its items.
    $wanted = '${wanted}' | ConvertFrom-Json
    $wanted = @($wanted | ForEach-Object { $_ })
    $until = (Get-Date).AddMilliseconds(${ms})
    do {
      $left = @()
      foreach ($w in $wanted) {
        $p = Get-Process -Id $w.pid -ErrorAction SilentlyContinue
        if (-not $p) { continue }
        # Opening the process keeps its id from going to a newer process while it is compared and ended. It is the
        # same process only when its start time matches the listing's too, so a newer one is neither waited on nor
        # ended.
        try { $null = $p.Handle; $same = [Math]::Abs(($p.StartTime - [datetime]$w.started).TotalMilliseconds) -lt 1 } catch { $same = $false }
        if (-not $same) { continue }
        $p.Refresh()
        if ($p.HandleCount -eq 0) { continue }
        $left += $w
        # Ending one already ended does nothing.
        try { $p.Kill() } catch {}
      }
      if ($left.Count -eq 0) { break }
      Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $until)
    ConvertTo-Json -Compress -InputObject $left`,
    ms,
  );
}

/**
 * Whether the process has ended, or never started, as Node saw it. Never by its process id alone: Windows gives an
 * ended process's id to the next process, and ending that would end a stranger's.
 */
function ended(child: ChildProcess): boolean {
  return child.pid === undefined || child.exitCode !== null || child.signalCode !== null;
}

/** Another window, as Core sees one: a second connection to the test's Core that sends one request after the handshake. */
export async function otherWindow(label: string, method: string, params: unknown): Promise<unknown> {
  const pipe = corePipe(label);
  if (!pipe) throw new Error(`no pipe for ${label}`);
  const socket = connect(pipe);
  const send = (id: number, m: string, p: unknown) => socket.write(`${JSON.stringify({ jsonrpc: "2.0", id, method: m, params: p })}\n`);
  return new Promise((resolve, reject) => {
    let buffered = "";
    socket.on("error", reject);
    socket.on("data", (chunk) => {
      buffered += chunk.toString("utf8");
      let end: number;
      while ((end = buffered.indexOf("\n")) >= 0) {
        const line = JSON.parse(buffered.slice(0, end));
        buffered = buffered.slice(end + 1);
        if (line.id === 1) send(2, method, params);
        if (line.id === 2) {
          socket.end();
          if (line.error) reject(new Error(JSON.stringify(line.error)));
          else resolve(line.result);
        }
      }
    });
    // Any version Core speaks will do for one request, so a check outlives a version bump.
    send(1, "core.hello", { client: "live-check", client_version: "0", api_versions: Array.from({ length: 32 }, (_, i) => i + 1) });
  });
}

/** The pipe Core serves `label` on, if it does: `ersa-core-<user SID>-<label>` (PROTO-CORE-API, Transport and framing). */
export function corePipe(label: string): string | undefined {
  const pipe = new RegExp(`^ersa-core-S-[0-9-]+-${label}$`);
  const name = readdirSync("\\\\.\\pipe\\").find((n) => pipe.test(n));
  return name === undefined ? undefined : `\\\\.\\pipe\\${name}`;
}

async function debuggerUp(port: number): Promise<boolean> {
  try {
    // Bounded, so a port that accepts and never answers cannot hold the start past its deadline.
    return (await fetch(`http://127.0.0.1:${port}/json/version`, { signal: AbortSignal.timeout(2_000) })).ok;
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

/** `work`, or an error once the start's deadline passes, for steps that take no timeout of their own. */
async function within<T>(deadline: number, work: Promise<T>, what: string): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  const late = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what} took past the start's ${START_LIMIT_MS / 1000} s`)), remaining(deadline));
  });
  try {
    return await Promise.race([work, late]);
  } finally {
    clearTimeout(timer);
  }
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
