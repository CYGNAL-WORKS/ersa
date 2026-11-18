import { expect, test } from "./harness";

// Closing the window (FLOW-CLOSE-AND-QUIT-R01): the system's close message, as the window's close button sends,
// hides the window; the Shell and Core keep running, and the Interface stays attached to Core.
test("closing the window hides it, and Ersa and Core keep running", async ({ ersa }) => {
  const { page } = ersa;
  const rail = page.getByRole("navigation", { name: "Rail" });
  await expect(rail.getByText(`Attached to ${ersa.label}`)).toBeVisible();
  expect(await ersa.windowShown()).toBe(true);

  await ersa.closeWindow();
  await expect.poll(() => ersa.windowShown(), { timeout: 10_000 }).toBe(false);
  expect(ersa.running()).toBe(true);
  // The Interface is still attached: its foot reports no lost connection.
  await expect(rail.getByText(`Attached to ${ersa.label}`)).toBeAttached();
  await expect(rail.getByRole("alert")).toHaveCount(0);
});
