import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BridgeError, CoreClient } from "../core/client";
import type { RawLogEntry, SessionRawLogParams } from "../protocol/core-api.gen";
import { RAW_LOG_CLIP, RAW_LOG_HELD, type RawLogView, readRawLog } from "./rawLog";

/** One `session.raw_log` call the fake client got, which the test answers or fails. */
interface Read {
  params: SessionRawLogParams;
  answer: (entries: unknown) => Promise<void>;
  fail: (error: BridgeError) => Promise<void>;
}

/** A client that holds every `session.raw_log` call until the test answers it. */
function fakeClient() {
  const reads: Read[] = [];
  const client: CoreClient = {
    attach: () => new Promise(() => {}),
    call: ((method: string, params: SessionRawLogParams) => {
      if (method !== "session.raw_log") return Promise.reject({ kind: "unavailable", message: `no answer for ${method}` });
      return new Promise<unknown>((resolve, reject) => {
        reads.push({
          params,
          // Each settles the promise, then lets the reader's continuation run.
          answer: async (entries) => {
            resolve(entries);
            await vi.advanceTimersByTimeAsync(0);
          },
          fail: async (error) => {
            reject(error);
            await vi.advanceTimersByTimeAsync(0);
          },
        });
      });
    }) as CoreClient["call"],
  };
  return { client, reads };
}

const entry = (index: number): RawLogEntry => ({ index, ts: "2026-11-22T10:00:00.000Z", dir: "backend", line: `line ${index}` });
/** Entries `from` up to but not including `to`. */
const entries = (from: number, to: number) => Array.from({ length: to - from }, (_, i) => entry(from + i));

/** A reader over a fake client, and every view it reported. */
function setUp() {
  const { client, reads } = fakeClient();
  const seen: (RawLogView | undefined)[] = [];
  const reader = readRawLog(client, (view) => seen.push(view));
  const last = () => seen.at(-1);
  const indices = () => last()?.lines.map((l) => l.index) ?? [];
  return { reader, reads, seen, last, indices };
}

const unreachable: BridgeError = { kind: "unavailable", message: "Core isn't running." };

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("reading a session's raw log", () => {
  // verifies: FLOW-READ-RAW-LOG-R11
  it("asks from the line after the last it holds, with the clip", async () => {
    const { reader, reads, indices } = setUp();
    reader.show({ open: true, session: "s1", link: 1 });
    expect(reads).toHaveLength(1);
    // Nothing held: from the first line Core holds, cut at 256 KiB (FLOW-READ-RAW-LOG-R11).
    expect(reads[0]?.params).toEqual({ id: "s1", clip: 262144 });
    expect(RAW_LOG_CLIP).toBe(262144);
    await reads[0]?.answer(entries(0, 3));
    expect(indices()).toEqual([0, 1, 2]);
    expect(reads[1]?.params).toEqual({ id: "s1", from: 3, clip: 262144 });
    await reads[1]?.answer(entries(3, 5));
    expect(indices()).toEqual([0, 1, 2, 3, 4]);
    expect(reads[2]?.params).toEqual({ id: "s1", from: 5, clip: 262144 });
  });

  it("asks again at once after a read that added a line, and a second after asking for one that added none", async () => {
    // verifies: FLOW-READ-RAW-LOG-R19
    const { reader, reads } = setUp();
    // The first read goes at once.
    reader.show({ open: true, session: "s1", link: 1 });
    expect(reads).toHaveLength(1);
    // One cut line, far short of a full answer, still adds a line: the next read goes at once.
    await reads[0]?.answer([{ index: 0, ts: "2026-11-22T10:00:00.000Z", dir: "host", head: "{", size: 300_000 }]);
    expect(reads).toHaveLength(2);
    expect(reads[1]?.params).toEqual({ id: "s1", from: 1, clip: 262144 });

    // A read that adds none: the next is due a second after that one was asked for, not after its answer.
    await vi.advanceTimersByTimeAsync(400);
    await reads[1]?.answer([]);
    await vi.advanceTimersByTimeAsync(599);
    expect(reads).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(reads).toHaveLength(3);
    expect(reads[2]?.params).toEqual({ id: "s1", from: 1, clip: 262144 });

    // A failed read adds none either.
    await reads[2]?.fail(unreachable);
    await vi.advanceTimersByTimeAsync(999);
    expect(reads).toHaveLength(3);
    await vi.advanceTimersByTimeAsync(1);
    expect(reads).toHaveLength(4);

    // A read still out when its second is up: never two out, and the next goes when it ends.
    await vi.advanceTimersByTimeAsync(3_000);
    expect(reads).toHaveLength(4);
    await reads[3]?.answer([]);
    expect(reads).toHaveLength(5);
    expect(reads[4]?.params).toEqual({ id: "s1", from: 1, clip: 262144 });
  });

  it("knows when the first answer for a session has come", async () => {
    const { reader, reads, last } = setUp();
    reader.show({ open: true, session: "s1", link: 1 });
    expect(last()).toEqual({ session: "s1", lines: [], read: false, earlier: false });
    await reads[0]?.answer([]);
    expect(last()).toEqual({ session: "s1", lines: [], read: true, earlier: false });
  });

  it("is still reading after a read that failed, until an answer comes", async () => {
    // verifies: FLOW-READ-RAW-LOG-R14
    const { reader, reads, last } = setUp();
    reader.show({ open: true, session: "s1", link: 1 });
    await reads[0]?.fail(unreachable);
    expect(last()).toMatchObject({ read: false, failure: "Core isn't running." });
    await vi.advanceTimersByTimeAsync(1_000);
    await reads[1]?.answer("not a list");
    expect(last()).toMatchObject({ read: false, failure: "Ersa got a raw log it can't read." });
    await vi.advanceTimersByTimeAsync(1_000);
    await reads[2]?.answer([]);
    expect(last()).toEqual({ session: "s1", lines: [], read: true, earlier: false });
  });

  it("asks for nothing while the workbench is closed", async () => {
    // verifies: FLOW-READ-RAW-LOG-R06
    const { reader, reads, last } = setUp();
    reader.show({ open: false, session: "s1", link: 1 });
    await vi.advanceTimersByTimeAsync(5_000);
    expect(reads).toHaveLength(0);
    expect(last()).toBeUndefined();

    // Opened, then closed with a read out and the next one due: no further read, and the late answer is dropped.
    reader.show({ open: true, session: "s1", link: 1 });
    await reads[0]?.answer(entries(0, 2));
    await vi.advanceTimersByTimeAsync(1_000);
    expect(reads).toHaveLength(2);
    reader.show({ open: false, session: "s1", link: 1 });
    expect(last()).toBeUndefined();
    await reads[1]?.answer(entries(2, 4));
    await vi.advanceTimersByTimeAsync(5_000);
    expect(reads).toHaveLength(2);
    expect(last()).toBeUndefined();

    // Open again: from the first line Core holds.
    reader.show({ open: true, session: "s1", link: 1 });
    expect(reads[2]?.params).toEqual({ id: "s1", clip: 262144 });
  });

  it("asks for nothing with no active session, and nothing once stopped", async () => {
    // verifies: FLOW-READ-RAW-LOG-R06
    const { reader, reads, last } = setUp();
    reader.show({ open: true, session: undefined, link: 1 });
    await vi.advanceTimersByTimeAsync(5_000);
    expect(reads).toHaveLength(0);
    expect(last()).toBeUndefined();
    reader.show({ open: true, session: "s1", link: 1 });
    await reads[0]?.answer([]);
    reader.stop();
    await vi.advanceTimersByTimeAsync(5_000);
    expect(reads).toHaveLength(1);
  });

  it("reads the new active session from the start, keeping nothing of the one it left", async () => {
    // verifies: FLOW-READ-RAW-LOG-R04
    const { reader, reads, last, indices } = setUp();
    reader.show({ open: true, session: "s1", link: 1 });
    await reads[0]?.answer(entries(0, 3));
    await vi.advanceTimersByTimeAsync(1_000);
    expect(reads).toHaveLength(2);

    reader.show({ open: true, session: "s2", link: 1 });
    // At once, from the first line Core holds, with no line of s1.
    expect(reads[2]?.params).toEqual({ id: "s2", clip: 262144 });
    expect(last()).toEqual({ session: "s2", lines: [], read: false, earlier: false });
    // The read of s1 still out answers late, and is dropped.
    await reads[1]?.answer(entries(3, 5));
    expect(last()?.session).toBe("s2");
    expect(indices()).toEqual([]);
    await reads[2]?.answer([{ ...entry(0), line: "s2's first" }]);
    expect(last()).toMatchObject({ session: "s2", read: true });
    expect(last()?.lines).toEqual([{ ...entry(0), line: "s2's first" }]);
    // Its next read is a second after its first, not after s1's.
    await vi.advanceTimersByTimeAsync(1_000);
    expect(reads[3]?.params).toEqual({ id: "s2", from: 1, clip: 262144 });
  });

  it("holds the newest 50,000 lines and knows earlier ones are gone", async () => {
    // verifies: FLOW-READ-RAW-LOG-R09
    const { reader, reads, last, indices } = setUp();
    expect(RAW_LOG_HELD).toBe(50_000);
    reader.show({ open: true, session: "s1", link: 1 });
    for (let page = 0; page < 50; page++) await reads[page]?.answer(entries(page * 1000, page * 1000 + 1000));
    expect(indices()).toHaveLength(50_000);
    expect(last()?.earlier).toBe(false);
    await reads[50]?.answer(entries(50_000, 50_400));
    expect(indices()).toHaveLength(50_000);
    expect(indices()[0]).toBe(400);
    expect(indices().at(-1)).toBe(50_399);
    expect(last()?.earlier).toBe(true);
  });

  it("knows earlier lines are gone when Core no longer holds the first", async () => {
    // verifies: FLOW-READ-RAW-LOG-R09
    const { reader, reads, last } = setUp();
    reader.show({ open: true, session: "s1", link: 1 });
    await reads[0]?.answer(entries(7, 9));
    expect(last()).toMatchObject({ earlier: true, read: true });
  });

  it("lists from an answer that starts later than asked, letting go of what it held", async () => {
    // verifies: FLOW-READ-RAW-LOG-R09
    const { reader, reads, last, indices } = setUp();
    reader.show({ open: true, session: "s1", link: 1 });
    await reads[0]?.answer(entries(0, 3));
    expect(last()?.earlier).toBe(false);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(reads[1]?.params.from).toBe(3);
    // Core let lines 3 to 9 go before this read.
    await reads[1]?.answer(entries(10, 12));
    expect(indices()).toEqual([10, 11]);
    expect(last()?.earlier).toBe(true);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(reads[2]?.params.from).toBe(12);
  });

  it("says a read failed with the reason, keeps its lines and goes on asking, until one succeeds", async () => {
    // verifies: FLOW-READ-RAW-LOG-R15
    const { reader, reads, last, indices } = setUp();
    reader.show({ open: true, session: "s1", link: 1 });
    await reads[0]?.answer(entries(0, 3));
    await vi.advanceTimersByTimeAsync(1_000);
    await reads[1]?.fail(unreachable);
    expect(last()).toMatchObject({ failure: "Core isn't running.", read: true });
    expect(indices()).toEqual([0, 1, 2]);

    // Still once a second, from the same place; a second failure keeps the message.
    await vi.advanceTimersByTimeAsync(1_000);
    expect(reads[2]?.params).toEqual({ id: "s1", from: 3, clip: 262144 });
    await reads[2]?.fail({ kind: "rpc", error: { code: -32000, message: "session.raw_log: nope", data: { kind: "internal", detail: "disk full" } } });
    expect(last()?.failure).toBe("disk full");
    await vi.advanceTimersByTimeAsync(1_000);
    await reads[3]?.answer(entries(3, 4));
    expect(last()).not.toHaveProperty("failure");
    expect(indices()).toEqual([0, 1, 2, 3]);
  });

  it("takes an answer it can't read as a failed read", async () => {
    // verifies: FLOW-READ-RAW-LOG-R15
    const { reader, reads, last, indices } = setUp();
    reader.show({ open: true, session: "s1", link: 1 });
    await reads[0]?.answer(entries(0, 2));
    for (const bad of [{ not: "a list" }, [{ index: 2, ts: "x", dir: "sideways", line: "l" }], [{ index: 2, ts: "x", dir: "backend" }], [entry(5), entry(4)]]) {
      await vi.advanceTimersByTimeAsync(1_000);
      await reads.at(-1)?.answer(bad);
      expect(last()?.failure).toBeTypeOf("string");
      expect(indices()).toEqual([0, 1]);
    }
    // A cut line, with its head and size, is a line it can read (FLOW-READ-RAW-LOG-R11).
    await vi.advanceTimersByTimeAsync(1_000);
    const cut = { index: 2, ts: "2026-11-22T10:00:00.000Z", dir: "host", head: "{\"type\":", size: 300_000 };
    await reads.at(-1)?.answer([cut]);
    expect(last()).not.toHaveProperty("failure");
    expect(last()?.lines.at(-1)).toEqual(cut);
  });

  it("reads from the first line Core holds when Ersa connects to Core again", async () => {
    // verifies: FLOW-READ-RAW-LOG-R18
    const { reader, reads, last, indices } = setUp();
    reader.show({ open: true, session: "s1", link: 1 });
    await reads[0]?.answer(entries(0, 5));
    await vi.advanceTimersByTimeAsync(1_000);
    expect(reads).toHaveLength(2);

    reader.show({ open: true, session: "s1", link: 2 });
    expect(reads[2]?.params).toEqual({ id: "s1", clip: 262144 });
    expect(last()).toEqual({ session: "s1", lines: [], read: false, earlier: false });
    // The read sent on the old connection answers late, and is dropped.
    await reads[1]?.answer(entries(5, 7));
    expect(indices()).toEqual([]);
    // A Core that restarted numbers from the start again.
    await reads[2]?.answer(entries(0, 2));
    expect(indices()).toEqual([0, 1]);

    // A lost connection keeps the lines; the next connection reads from the start.
    await vi.advanceTimersByTimeAsync(1_000);
    reader.show({ open: true, session: "s1", link: undefined });
    expect(indices()).toEqual([0, 1]);
    await reads[3]?.fail(unreachable);
    expect(indices()).toEqual([0, 1]);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(reads[4]?.params).toEqual({ id: "s1", from: 2, clip: 262144 });
    reader.show({ open: true, session: "s1", link: 3 });
    expect(reads[5]?.params).toEqual({ id: "s1", clip: 262144 });

    // The same values again change nothing.
    reader.show({ open: true, session: "s1", link: 3 });
    expect(reads).toHaveLength(6);

    // Try again starts a new connection that counts its links from 1 again, so the same number after none is a new
    // connection too.
    await reads[5]?.answer(entries(0, 3));
    expect(reads[6]?.params).toEqual({ id: "s1", from: 3, clip: 262144 });
    reader.show({ open: true, session: "s1", link: undefined });
    expect(indices()).toEqual([0, 1, 2]);
    reader.show({ open: true, session: "s1", link: 3 });
    expect(reads[7]?.params).toEqual({ id: "s1", clip: 262144 });
    expect(last()).toEqual({ session: "s1", lines: [], read: false, earlier: false });
  });
});
