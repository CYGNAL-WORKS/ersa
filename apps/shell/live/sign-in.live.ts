import { expect, test } from "./harness";

// The sign-in note against the real Core with the stand-in reporting signed out (FLOW-SIGN-IN). No adapter Core
// ships can start a sign-in yet (the Claude Code adapter's waits on a recording), so Sign in is refused by Core with
// the adapter's reason, which the note shows: a sign-in is never played here, and nothing is opened.
test.use({ signedOut: true });

test("a signed-out backend shows the sign-in note, and a start Core refuses says why", async ({ ersa }) => {
  const { page } = ersa;
  const screen = page.getByRole("region", { name: "New session" });
  await expect(page.locator(".sign-in-words")).toHaveText("Claude Code isn't signed in on this host. Sign in to start a session.");
  // The composer is unavailable while the note shows (FLOW-SIGN-IN-R01).
  await expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
  await ersa.shot("signed-out");
  await screen.getByRole("button", { name: "Sign in" }).click();
  // Core refuses: the note says it couldn't be started, with Core's reason, and offers Sign in again (R03).
  // announced as an alert, the same words shown in the note.
  await expect(screen.getByRole("alert")).toHaveText("Claude Code's sign-in couldn't be started. Ersa has no sign-in for Claude Code.");
  await expect(page.locator(".sign-in-words")).toHaveText("Claude Code's sign-in couldn't be started. Ersa has no sign-in for Claude Code.");
  await expect(screen.getByRole("button", { name: "Sign in" })).toBeEnabled();
  await ersa.shot("refused");
});
