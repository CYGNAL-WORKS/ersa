import { expect, test } from "./harness";

// A backend installed while Ersa is open (FLOW-NEW-SESSION-R50 to R52): with nothing where Core looks, the
// new-session screen says Ersa can't find Claude Code; the backend is put there, the window is closed to the tray and
// opened again, and the screen offers it, with nothing restarted. The window is opened by a second launch, as
// background windows never take the focus: the Shell says the window came back, which no event of the page does.
test.use({ installedLater: true });

test("a backend installed while Ersa is open is offered once the window comes back", async ({ ersa }) => {
  const { page } = ersa;
  const screen = page.getByRole("region", { name: "New session" });
  const statusLine = screen.locator(".composer-status-line");
  const backendChip = screen.getByRole("button", { name: /^Backend:/ });
  await expect(statusLine).toHaveText(
    "Ersa can't find Claude Code on this host. Install it, and Ersa looks again when you come back to this window.",
  );
  await expect(backendChip).toHaveAccessibleName("Backend: No backend");
  await backendChip.click();
  await expect(page.getByRole("option", { name: /Claude Code/ })).toContainText("Not found");
  await ersa.shot("cant-find-claude-code");
  await page.keyboard.press("Escape");

  ersa.installBackend();
  await ersa.closeWindow();
  await expect.poll(() => ersa.windowShown(), { timeout: 10_000 }).toBe(false);
  await ersa.openWindow();
  await expect.poll(() => ersa.windowShown(), { timeout: 10_000 }).toBe(true);

  await expect(backendChip).toHaveAccessibleName("Backend: Claude Code", { timeout: 15_000 });
  await expect(statusLine).toHaveCount(0);
  await expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled();
  await ersa.shot("claude-code-offered");
});
