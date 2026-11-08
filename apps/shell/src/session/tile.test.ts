// What a session's tile shows, from the session as Core lists it (LAY-SHELL, The rail: Tiles, State, Needs you,
// Context; docs/design/RAIL.md, A tile). LAY-SHELL is a layout brief and carries no requirement IDs.

import { describe, expect, it } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import type { BackendChoices } from "./controls";
import { contextUsed, nearFull, tileOf } from "./tile";

type Tile = Session["tile"];
type Pending = Tile["pending"][number];

const session = (tile: Partial<Tile> = {}, rest: Partial<Session> = {}): Session => ({
  id: "s",
  workspace_id: "w",
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  title: "Fix the build",
  state: tile.state ?? "running",
  created_at: "2026-11-01T09:00:00Z",
  archived: false,
  pinned: false,
  tile: { state: "running", pending: [], ...tile },
  ...rest,
});

const permission: Pending = { kind: "permission", request_id: "p" };
const question: Pending = { kind: "question", request_id: "q" };
// A kind a newer Core might send, which this interface doesn't know.
const unknownKind = { kind: "approval", request_id: "u" } as unknown as Pending;

describe("a tile's mark and phrase, by state", () => {
  it.each([
    ["starting", "starting", "accent", "Starting", false],
    ["running", "working", "accent", undefined, false],
    ["idle", undefined, undefined, undefined, true],
    ["disconnected", "disconnected", "faint", "Disconnected", false],
    ["error", "error", "danger", "Error", false],
    ["ended", undefined, undefined, "Ended", true],
  ] as const)("%s: mark %s in %s, phrase %s, title muted %s", (state, mark, tone, phrase, titleMuted) => {
    const view = tileOf(session({ state }));
    expect(view.mark).toBe(mark);
    expect(view.tone).toBe(tone);
    expect(view.phrase).toBe(phrase);
    expect(view.titleMuted).toBe(titleMuted);
  });

  it("puts an archive mark, named Archived, in place of the state's mark on an archived session, its title muted", () => {
    const view = tileOf(session({ state: "ended" }, { archived: true }));
    expect([view.mark, view.tone, view.markLabel, view.phrase, view.titleMuted]).toEqual(["archived", undefined, "Archived", "Ended", true]);
  });

  it("names the working mark, which has no phrase, for assistive technology", () => {
    expect(tileOf(session({ state: "running" })).markLabel).toBe("Working");
  });

  it("reads a session Core reports as waiting on the user, with nothing pending, as idle", () => {
    const view = tileOf(session({ state: "waiting_user", pending: [] }));
    expect([view.mark, view.tone, view.phrase, view.titleMuted]).toEqual([undefined, undefined, undefined, true]);
  });

  it("gives a state the interface doesn't know no mark, and says so", () => {
    const view = tileOf(session({ state: "paused" as unknown as Tile["state"] }));
    expect([view.mark, view.tone, view.phrase, view.titleMuted]).toEqual([undefined, undefined, "Unknown state", false]);
  });
});

describe("a tile that needs the user", () => {
  it("shows a permission request in caution", () => {
    const view = tileOf(session({ state: "waiting_user", pending: [permission] }));
    expect(view).toMatchObject({ mark: "needsYou", tone: "caution", phrase: "Needs permission", markLabel: "Needs permission", titleMuted: false });
  });

  it("shows a question in the accent", () => {
    expect(tileOf(session({ state: "waiting_user", pending: [question] }))).toMatchObject({
      mark: "needsYou",
      tone: "accent",
      phrase: "Has a question",
      markLabel: "Has a question",
    });
  });

  it("counts more than one request", () => {
    expect(tileOf(session({ pending: [question, question, question] }))).toMatchObject({ tone: "accent", phrase: "Needs you · 3" });
  });

  it("takes the permission request's tone with both kinds pending", () => {
    expect(tileOf(session({ pending: [question, permission] }))).toMatchObject({ tone: "caution", phrase: "Needs you · 2" });
  });

  it("reads a request of a kind it doesn't know as needing the user, in caution", () => {
    expect(tileOf(session({ pending: [unknownKind] }))).toMatchObject({ mark: "needsYou", tone: "caution", phrase: "Needs you" });
    expect(tileOf(session({ pending: [question, unknownKind] }))).toMatchObject({ tone: "caution", phrase: "Needs you · 2" });
  });

  it("lets the pending requests decide, whatever state Core reports", () => {
    for (const state of ["idle", "ended", "running", "error"] as const) {
      expect(tileOf(session({ state, pending: [permission] }))).toMatchObject({ mark: "needsYou", tone: "caution", titleMuted: false });
    }
  });
});

describe("a tile's values as reported", () => {
  it("shows the cost once one is reported, to the cent", () => {
    expect(tileOf(session({ cost_usd_total: 12.4 })).cost).toBe("$12.40");
    expect(tileOf(session({ cost_usd_total: 0 })).cost).toBe("$0.00");
    expect(tileOf(session({})).cost).toBeUndefined();
    expect(tileOf(session({ cost_usd_total: Number.NaN })).cost).toBeUndefined();
  });

  it("shows the ring only once the tokens used and a context window above zero are reported", () => {
    expect(tileOf(session({ context_tokens: 50_000, context_window: 200_000 })).ring).toBe(0.25);
    expect(tileOf(session({ context_tokens: 0, context_window: 200_000 })).ring).toBe(0);
    expect(tileOf(session({ context_window: 200_000 })).ring).toBeUndefined();
    expect(tileOf(session({ context_tokens: 50_000 })).ring).toBeUndefined();
    expect(tileOf(session({ context_tokens: 50_000, context_window: 0 })).ring).toBeUndefined();
  });

  it("never shows more than a full ring", () => {
    expect(tileOf(session({ context_tokens: 300_000, context_window: 200_000 })).ring).toBe(1);
  });

  it("names a session with no title as untitled", () => {
    expect(tileOf(session({}, { title: undefined })).title).toBe("Untitled session");
    expect(tileOf(session({}, { title: "" })).title).toBe("Untitled session");
    expect(tileOf(session({}, { title: "Fix the build" })).title).toBe("Fix the build");
  });

  it("names the backend by its own name, or its id while its name isn't known", () => {
    const backends = { local: [{ name: "claude-code", label: "Claude Code" } as BackendChoices] };
    expect(tileOf(session(), backends).backend).toBe("Claude Code");
    expect(tileOf(session()).backend).toBe("claude-code");
    expect(tileOf(session({}, { host_id: "devbox" }), backends).backend).toBe("claude-code");
  });
});

describe("the context ring near full (LAY-SHELL, Context)", () => {
  it("is near full from 85 percent", () => {
    expect(nearFull(0.844)).toBe(false);
    expect(nearFull(0.85)).toBe(true);
    // Read in whole percent, as the readout shows it: what reads "85%" is near full.
    expect(nearFull(0.846)).toBe(true);
    expect(nearFull(1)).toBe(true);
    expect(nearFull(undefined)).toBe(false);
  });

  it("reads the share used the same way for the session controls", () => {
    expect(contextUsed({ state: "idle", pending: [], context_tokens: 170_000, context_window: 200_000 })).toBe(0.85);
    expect(contextUsed({ state: "idle", pending: [] })).toBeUndefined();
  });
});
