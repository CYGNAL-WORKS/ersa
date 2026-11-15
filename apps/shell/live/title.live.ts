import { mkdirSync } from "node:fs";
import { connect } from "node:net";
import path from "node:path";
import { corePipe, expect, test } from "./harness";

// A session's title in the running window with real Claude Code (#329; PROTO-CORE-API-R83, R84): a new session's tile
// shows the name the backend generated during its first turn, with no reload, and a rename from another client shows at
// once, on the tile and in the transcript header.
test.use({ backend: "claude" });

/** Another window, as Core sees one: a second connection to the test's Core that sends one request after the handshake. */
async function otherWindow(label: string, method: string, params: unknown): Promise<unknown> {
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
    // Any version Core speaks will do for one rename, so this check outlives a version bump.
    send(1, "core.hello", { client: "live-check", client_version: "0", api_versions: Array.from({ length: 32 }, (_, i) => i + 1) });
  });
}

test("a new session's generated name and a rename from another window show without a reload", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "title-check");
  mkdirSync(folder);
  const rail = page.getByRole("navigation", { name: "Rail" });

  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(rail.getByRole("button", { name: /^New session in / })).toBeVisible();

  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("Write a short haiku about lighthouses. Do not use any tools.");
  await words.press("Enter");

  // The backend names the session during its first turn; the tile shows it with nothing reloaded.
  const tile = rail.locator("[data-session-id]").first();
  await expect(tile).toBeVisible({ timeout: 120_000 });
  await expect(tile).not.toContainText("Untitled session", { timeout: 120_000 });
  await ersa.shot("generated-name");
  const id = await tile.getAttribute("data-session-id");

  // Renamed from another window: this window shows it at once, on the tile and in the header.
  await otherWindow(ersa.label, "session.set_title", { id, title: "Renamed elsewhere" });
  await expect(tile).toContainText("Renamed elsewhere", { timeout: 30_000 });
  await expect(page.getByRole("heading", { name: "Renamed elsewhere" })).toBeVisible();
  await ersa.shot("renamed-elsewhere");
});
