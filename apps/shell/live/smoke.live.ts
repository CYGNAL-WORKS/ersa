import { expect, test } from "./harness";

// The harness itself (#207): the window opens against this test's Core and data folder, shows the new-session
// screen, and its rail's foot reports no problem, only the label it is attached to (LAY-SHELL, Development).
test("the window opens on the new-session screen against its own Core", async ({ ersa }) => {
  const { page } = ersa;
  await expect(page.getByRole("region", { name: "New session" })).toBeVisible();
  const rail = page.getByRole("navigation", { name: "Rail" });
  await expect(rail.getByText(`Attached to ${ersa.label}`)).toBeVisible();
  // The foot's status line is empty and it raises no alert: Ersa reached Core.
  await expect(rail.getByRole("status")).toHaveText("");
  await expect(rail.getByRole("alert")).toHaveCount(0);
  await ersa.shot("new-session");
});
