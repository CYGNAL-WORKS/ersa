import { afterEach, describe, expect, it, vi } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import type { CoreClient } from "./client";
import { QUIT_LIST_TIMEOUT_MS, type ShellControl, answerQuit, busySessions, cancelQuit } from "./shell";

const inState = (id: string, state: Session["tile"]["state"]): Session => ({
  id,
  workspace_id: "w",
  host_id: "local",
  backend: "claude-code",
  state,
  created_at: "2026-11-01T09:00:00.000Z",
  tile: { state, pending: [] },
});

function fakes(listed: Session[] | Error | "never", wasHidden = true) {
  const client = {
    attach: vi.fn(),
    call: vi.fn(() => {
      if (listed === "never") return new Promise(() => {});
      if (listed instanceof Error) return Promise.reject({ kind: "unavailable", message: listed.message });
      return Promise.resolve(listed);
    }),
  } as unknown as CoreClient;
  const shell: ShellControl = {
    onQuitRequested: vi.fn(() => () => {}),
    quitHeard: vi.fn(async () => {}),
    show: vi.fn(async () => wasHidden),
    hide: vi.fn(async () => {}),
    quit: vi.fn(async () => {}),
  };
  return { client, shell };
}

afterEach(() => vi.useRealTimers());

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
    expect(await answerQuit(client, shell)).toBeUndefined();
    expect(shell.quitHeard).toHaveBeenCalledTimes(1);
    expect(shell.quit).toHaveBeenCalledTimes(1);
    expect(shell.show).not.toHaveBeenCalled();
  });

  // verifies: FLOW-CLOSE-AND-QUIT-R04
  it("asks first when a session is working or waiting, and sends nothing until the user confirms", async () => {
    const { client, shell } = fakes([inState("a", "running"), inState("b", "waiting_user"), inState("c", "idle")]);
    expect(await answerQuit(client, shell)).toEqual({ busy: 2, wasHidden: true });
    expect(shell.show).toHaveBeenCalledTimes(1);
    expect(shell.quit).not.toHaveBeenCalled();
    // Only the list was read: nothing that stops or changes a session.
    expect(vi.mocked(client.call).mock.calls).toEqual([["session.list"]]);
  });

  // verifies: FLOW-CLOSE-AND-QUIT-R04
  it("cancelling puts a window that was in the tray back there, and sends nothing to Core", async () => {
    const { client, shell } = fakes([inState("a", "running")]);
    const ask = await answerQuit(client, shell);
    expect(ask).toBeDefined();
    await cancelQuit(shell, ask!);
    expect(shell.hide).toHaveBeenCalledTimes(1);
    expect(shell.quit).not.toHaveBeenCalled();
    expect(vi.mocked(client.call).mock.calls).toEqual([["session.list"]]);

    const open = fakes([inState("a", "running")], false);
    await cancelQuit(open.shell, (await answerQuit(open.client, open.shell))!);
    expect(open.shell.hide).not.toHaveBeenCalled();
  });

  // verifies: FLOW-CLOSE-AND-QUIT-R04
  it("counts the rail's last list when Core cannot be asked", async () => {
    const { client, shell } = fakes(new Error("Core closed the connection"));
    expect(await answerQuit(client, shell, [inState("a", "running")])).toEqual({ busy: 1, wasHidden: true });
    expect(shell.quit).not.toHaveBeenCalled();
  });

  // verifies: FLOW-CLOSE-AND-QUIT-R04
  it("asks anyway when the sessions cannot be counted at all", async () => {
    const { client, shell } = fakes(new Error("not attached to Core"));
    expect(await answerQuit(client, shell, undefined)).toEqual({ busy: undefined, wasHidden: true });
    expect(shell.quit).not.toHaveBeenCalled();
    expect(shell.show).toHaveBeenCalledTimes(1);
  });

  // verifies: FLOW-CLOSE-AND-QUIT-R04
  it("counts the rail's last list when Core is slow to answer", async () => {
    vi.useFakeTimers();
    const { client, shell } = fakes("never");
    const answer = answerQuit(client, shell, [inState("a", "waiting_user")]);
    await vi.advanceTimersByTimeAsync(QUIT_LIST_TIMEOUT_MS);
    expect(await answer).toEqual({ busy: 1, wasHidden: true });
  });
});
