import path from "node:path";
import { expect, test } from "./harness";

// The harness answers what Playwright cannot reach (#207): a system dialog and a drop from the file manager. Here the
// window's own script asks, exactly as the Interface does through Tauri's API, and the check reads what the window
// received; attach.live.ts and paste-drop.live.ts drive the composer's own dialogs and drops.

test("a system dialog is answered from the test", async ({ ersa }) => {
  const { page } = ersa;
  const folder = path.join(ersa.dir, "picked");
  const answered = ersa.answerDialog(folder);
  // `open({ directory: true })` from @tauri-apps/plugin-dialog makes this call.
  const received = await page.evaluate(() =>
    (window as unknown as TauriWindow).__TAURI_INTERNALS__.invoke("plugin:dialog|open", { options: { directory: true } }),
  );
  expect(received).toBe(folder);
  expect(await answered).toEqual({ command: "open", options: { directory: true } });

  // Cancel answers null.
  const canceled = ersa.answerDialog(null);
  const nothing = await page.evaluate(() =>
    (window as unknown as TauriWindow).__TAURI_INTERNALS__.invoke("plugin:dialog|open", { options: { multiple: true } }),
  );
  expect(nothing).toBeNull();
  expect((await canceled).options).toEqual({ multiple: true });
});

test("a drop from the file manager is answered from the test", async ({ ersa }) => {
  const { page } = ersa;
  // Listen as `getCurrentWebview().onDragDropEvent` from @tauri-apps/api does: through the Shell's event listener.
  await page.evaluate(async () => {
    const tauri = (window as unknown as TauriWindow).__TAURI_INTERNALS__;
    const seen: unknown[] = [];
    (window as unknown as { seen: unknown[] }).seen = seen;
    for (const event of ["tauri://drag-enter", "tauri://drag-over", "tauri://drag-drop"]) {
      await tauri.invoke("plugin:event|listen", {
        event,
        target: { kind: "Webview", label: "main" },
        handler: tauri.transformCallback((e: { event: string; payload: unknown }) => seen.push([e.event, e.payload])),
      });
    }
  });
  const files = [path.join(ersa.dir, "notes.md"), path.join(ersa.dir, "src")];
  const rail = page.getByRole("navigation", { name: "Rail" });
  await ersa.drop(files, rail);

  const seen = (await page.evaluate(() => (window as unknown as { seen: unknown[] }).seen)) as [string, { paths?: string[]; position: { x: number; y: number } }][];
  expect(seen.map(([event]) => event)).toEqual(["tauri://drag-enter", "tauri://drag-over", "tauri://drag-drop"]);
  const [, dropped] = seen[2]!;
  expect(dropped.paths).toEqual(files);
  // Over the rail, in physical pixels.
  const box = (await rail.boundingBox())!;
  const scale = await page.evaluate(() => window.devicePixelRatio);
  expect(dropped.position.x).toBeGreaterThan(box.x * scale);
  expect(dropped.position.x).toBeLessThan((box.x + box.width) * scale);
});

interface TauriWindow {
  __TAURI_INTERNALS__: {
    invoke(command: string, args?: unknown): Promise<unknown>;
    transformCallback(callback: (value: never) => void): number;
  };
}
