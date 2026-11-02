import { describe, expect, it, vi } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import type { CoreClient } from "./client";
import { type ShellControl, answerQuit, busySessions } from "./shell";

const inState = (id: string, state: Session["tile"]["state"]): Session => ({
  id,
  workspace_id: "w",
  host_id: "local",
  backend: "claude-code",
  state,
  created_at: "2026-11-01T09:00:00.000Z",
  tile: { state, pending: [] },
});

function fakes(listed: Session[] | Error) {
  const client = {
    attach: vi.fn(),
    call: vi.fn(async () => {
      if (listed instanceof Error) throw { kind: "unavailable", message: listed.message };
      return listed;
    }),
  } as unknown as CoreClient;
  const shell: ShellControl = { onQuitRequested: vi.fn(() => () => {}), show: vi.fn(async () => {}), quit: vi.fn(async () => {}) };
  return { client, shell };
}

describe("quitting", () => {
  // verifies: FLOW-CLOSE-AND-QUIT-R04
  it("counts working, starting and waiting sessions, and no others", () => {
    const sessions = [
      inState("a", "running"),
      inState("b", "waiting_user"),
      inState("c", "starting"),
      inState("d", "idle"),
      inState("e", "ended"),
      inState("f", "error"),
      inState("g", "disconnected"),
    ];
    expect(busySessions(sessions)).toBe(3);
  });

  // verifies: FLOW-CLOSE-AND-QUIT-R04
  it("quits at once when no session is busy", async () => {
    const { client, shell } = fakes([inState("a", "idle"), inState("b", "ended")]);
    expect(await answerQuit(client, shell)).toBe(0);
    expect(shell.quit).toHaveBeenCalledTimes(1);
    expect(shell.show).not.toHaveBeenCalled();
  });

  // verifies: FLOW-CLOSE-AND-QUIT-R04
  it("asks first when a session is working or waiting, and sends nothing until the user confirms", async () => {
    const { client, shell } = fakes([inState("a", "running"), inState("b", "waiting_user"), inState("c", "idle")]);
    expect(await answerQuit(client, shell)).toBe(2);
    expect(shell.show).toHaveBeenCalledTimes(1);
    expect(shell.quit).not.toHaveBeenCalled();
    // Only the list was read: nothing that stops or changes a session.
    expect(vi.mocked(client.call).mock.calls).toEqual([["session.list"]]);
  });

  // verifies: FLOW-CLOSE-AND-QUIT-R04
  it("counts the rail's last list when Core cannot be asked", async () => {
    const { client, shell } = fakes(new Error("Core closed the connection"));
    expect(await answerQuit(client, shell, [inState("a", "running")])).toBe(1);
    expect(shell.quit).not.toHaveBeenCalled();
  });
});
