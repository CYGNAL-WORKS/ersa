// @vitest-environment jsdom
// The rail's arrangement as the window holds it while Core saves each change (FLOW-ORGANIZE-SESSIONS, Saving fails).

import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { type Patch, choicePatch, useRailArrangement } from "./arrangement";

/** A save Core answers when the test says, recording each patch it was sent. */
function core() {
  const sent: Patch[] = [];
  const answers: { ok: () => void; fail: (e: Error) => void }[] = [];
  const save = (patch: Patch) =>
    new Promise<unknown>((resolve, reject) => {
      sent.push(patch);
      answers.push({ ok: () => resolve({}), fail: reject });
    });
  return { sent, answers, save };
}
const describeError = (e: unknown) => (e as Error).message;

describe("the rail's arrangement while Core saves it", () => {
  // verifies: FLOW-ORGANIZE-SESSIONS-R13
  it("is read from Core's settings, and is reading until then", () => {
    const { save } = core();
    const { result } = renderHook(() => useRailArrangement(save, describeError));
    expect(result.current.reading).toBe(true);
    act(() => result.current.read({ rail: { group_by: "none", show: "archived", collapsed: { none: ["pinned"] } } }));
    expect(result.current.reading).toBe(false);
    expect(result.current.arrangement).toEqual({ groupBy: "none", show: "archived", collapsed: { none: ["pinned"] } });
  });

  it("changes at once, sends the change to Core, and keeps it once Core has it", async () => {
    const c = core();
    const { result } = renderHook(() => useRailArrangement(c.save, describeError));
    act(() => result.current.read({}));
    act(() => result.current.change(choicePatch({ groupBy: "date" })));
    expect(result.current.arrangement.groupBy).toBe("date");
    expect(c.sent).toEqual([{ rail: { group_by: "date" } }]);
    await act(async () => c.answers[0]!.ok());
    expect(result.current.arrangement.groupBy).toBe("date");
    expect(result.current.problem).toBeUndefined();
  });

  it("undoes a change Core refuses and says why, keeping the changes Core saved, until the next change", async () => {
    const c = core();
    const { result } = renderHook(() => useRailArrangement(c.save, describeError));
    act(() => result.current.read({}));
    act(() => result.current.change(choicePatch({ show: "all" })));
    act(() => result.current.change(choicePatch({ groupBy: "none" })));
    await act(async () => c.answers[1]!.fail(new Error("disk full")));
    expect(result.current.arrangement).toMatchObject({ groupBy: "project", show: "all" });
    expect(result.current.problem).toEqual({ kind: "save", message: "disk full" });
    await act(async () => c.answers[0]!.ok());
    expect(result.current.arrangement).toMatchObject({ groupBy: "project", show: "all" });
    act(() => result.current.change(choicePatch({ groupBy: "date" })));
    expect(result.current.problem).toBeUndefined();
  });

  it("arranges by the defaults and says why when Core's settings can't be read", () => {
    const { save } = core();
    const { result } = renderHook(() => useRailArrangement(save, describeError));
    act(() => result.current.readFailed("Core is away"));
    expect(result.current.reading).toBe(false);
    expect(result.current.arrangement).toEqual({ groupBy: "project", show: "active", collapsed: {} });
    expect(result.current.problem).toEqual({ kind: "read", message: "Core is away" });
  });

  it("stops saying the settings can't be read once a later connection reads them, but keeps saying a change was refused", async () => {
    const { save, answers } = core();
    const { result } = renderHook(() => useRailArrangement(save, describeError));
    act(() => result.current.readFailed("Core is away"));
    act(() => result.current.read({ rail: { group_by: "date" } }));
    expect(result.current.problem).toBeUndefined();
    expect(result.current.arrangement.groupBy).toBe("date");
    act(() => result.current.change(choicePatch({ show: "all" })));
    await act(async () => answers[0]!.fail(new Error("disk full")));
    act(() => result.current.read({}));
    expect(result.current.problem).toEqual({ kind: "save", message: "disk full" });
  });
});
