import { describe, expect, it } from "vitest";
import type { CoreClient, Method } from "../core/client";
import type { Session } from "../protocol/core-api.gen";
import { NO_CHANGES, changeAnswered, changeSent, changeSession, refusalSeen, runs } from "./change";

const tile = (state: Session["tile"]["state"]) => ({ tile: { state, pending: [] } });

describe("whether a session's controls change it", () => {
  // verifies: FLOW-NEW-SESSION-R30
  it("only while it runs in Core", () => {
    for (const state of ["starting", "running", "waiting_user", "idle"] as const) expect(runs(tile(state))).toBe(true);
    for (const state of ["ended", "error", "disconnected"] as const) expect(runs(tile(state))).toBe(false);
  });
});

describe("changing a running session", () => {
  const session = { model: "claude-sonnet-5-5", model_request: "sonnet", effort: "high", permission_mode: "plan", title: "Kept", tile: { state: "idle", pending: [] } } as unknown as Session;
  const client = (answer: (method: Method, params: unknown) => unknown): Pick<CoreClient, "call"> => ({
    call: ((method: Method, params: unknown) => new Promise((resolve) => resolve(answer(method, params) as never))) as CoreClient["call"],
  });

  // verifies: FLOW-NEW-SESSION-R30
  it.each([
    ["model", "session.set_model", { id: "s1", model: "sonnet" }],
    ["effort", "session.set_effort", { id: "s1", effort: "sonnet" }],
    ["permission_mode", "session.set_permission_mode", { id: "s1", permission_mode: "sonnet" }],
  ] as const)("sends a %s change by its method and answers only the fields a change moves", async (key, method, params) => {
    const calls: unknown[] = [];
    const changed = await changeSession(client((m, p) => (calls.push([m, p]), session)), "s1", key, "sonnet");
    expect(calls).toEqual([[method, params]]);
    expect(changed).toEqual({ model: "claude-sonnet-5-5", model_request: "sonnet", effort: "high", permission_mode: "plan" });
  });

  // verifies: FLOW-NEW-SESSION-R30
  it("says why Core refused, after what could not change", async () => {
    const refused = client(() => {
      throw { kind: "rpc", error: { code: -32000, message: "session.set_model: the session is not live", data: { kind: "invalid_state" } } };
    });
    await expect(changeSession(refused, "s1", "model", "sonnet")).rejects.toThrow("Ersa can't change the model. The session is not live.");
  });
});

describe("a session's changes on their way", () => {
  // verifies: FLOW-NEW-SESSION-R30
  it("keep a refusal's line when another change is accepted later, until the user sends or opens a chip", () => {
    let c = changeSent(NO_CHANGES, "effort");
    c = changeSent(c, "model");
    c = changeAnswered(c, "model", "Ersa can't change the model. No.");
    expect(c).toEqual({ sending: ["effort"], refused: "Ersa can't change the model. No." });
    c = changeAnswered(c, "effort");
    expect(c).toEqual({ sending: [], refused: "Ersa can't change the model. No." });
    expect(refusalSeen(c)).toEqual({ sending: [] });
  });

  // verifies: FLOW-NEW-SESSION-R30
  it("hold each chip on its own while its change is on its way", () => {
    let c = changeSent(changeSent(NO_CHANGES, "model"), "effort");
    c = changeAnswered(c, "effort");
    // The model's change is still on its way, so its chip still waits.
    expect(c.sending).toEqual(["model"]);
    expect(refusalSeen(c).sending).toEqual(["model"]);
  });
});
