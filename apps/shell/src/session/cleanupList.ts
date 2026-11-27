// The round trips behind the Clean up list (FLOW-CLEAN-UP, steps 5 to 7; R17 to R19): `cleanup.list` when Storage
// shows and when the days change, `cleanup.inspect` for each item a few at a time, and `cleanup.remove` for each
// ticked item one at a time once the user confirms. What is ticked and what the question says are `cleanup.ts`'s.

import { useCallback, useMemo, useRef, useState } from "react";
import { type CoreClient, refusalOf } from "../core/client";
import type { CleanupInspectResult, CleanupItem, CleanupListResult } from "../protocol/core-api.gen";
import { t } from "../catalog";
import { type CleanUpDays, type ItemView, changedTo, sendsDiscard, tickedByDefault } from "./cleanup";

/**
 * How many items are inspected at once. Each inspect holds one of Core's eight places for up to 30 seconds, and with
 * none free other requests fail at once (PROTO-CORE-API, Clean up), so the list takes a few and leaves the rest.
 */
export const INSPECT_AT_ONCE = 3;

/** The list as read: being read, read, or not readable and why. */
export type ListView =
  | { state: "loading" }
  | { state: "error"; message: string }
  | { state: "ready"; items: ItemView[]; unread: CleanupListResult["unread"] };

/** The question and the removals behind it: open, removing the ticked items in turn, or stopping after the current one. */
export type RunView =
  | { state: "asking"; ticked: CleanupInspectResult[] }
  | { state: "removing"; ticked: CleanupInspectResult[]; done: number; stopping: boolean };

/** What the last clean-up did, said above the list until the next one or a change of days (R18). */
export interface Result {
  removed: number;
  count: number;
  stopped: boolean;
  /** Each item not removed, as it was when it was ticked, and why. */
  failures: { item: CleanupItem; message: string }[];
}

export interface CleanUpView {
  days: CleanUpDays;
  list: ListView;
  /** The ids ticked now: the user's own choices over each item's default (R16). */
  ticked: ReadonlySet<string>;
  run?: RunView;
  result?: Result;
}

export interface CleanUpActions {
  /** A choice of days, which reads the list again. */
  choose: (days: CleanUpDays) => void;
  tick: (id: string, on: boolean) => void;
  /** Clean up…: opens the question over what is ticked. */
  ask: () => void;
  cancel: () => void;
  /** The question confirmed: the removals begin. */
  confirm: () => void;
  /** Stop: the item being removed finishes, and no other begins. */
  stop: () => void;
}

/** The ticked items as they were inspected, in list order. */
function tickedOf(list: ListView, ticked: ReadonlySet<string>): CleanupInspectResult[] {
  if (list.state !== "ready") return [];
  return list.items.flatMap((v) => (v.state === "inspected" && ticked.has(v.item.id) ? [v.inspected] : []));
}

export function useCleanUp(client: CoreClient, describe: (error: unknown) => string) {
  const [days, setDays] = useState<CleanUpDays>(30);
  const [list, setList] = useState<ListView>({ state: "loading" });
  // The user's own ticks, by id; an item they haven't touched takes its default.
  const [choices, setChoices] = useState<Readonly<Record<string, boolean>>>({});
  const [run, setRun] = useState<RunView>();
  const [result, setResult] = useState<Result>();
  // Each read has a number; an answer to an older read, or an inspect it began, changes nothing.
  const reading = useRef(0);
  const stopping = useRef(false);
  // The items waiting for an inspect, the read they belong to, and how many inspects are at Core now. One pool for
  // every read: an inspect already sent can't be called off, so a read made meanwhile waits for its place rather than
  // adding more, and never more than INSPECT_AT_ONCE are at Core whatever the reads.
  const waiting = useRef<{ read: number; items: CleanupItem[] }>({ read: 0, items: [] });
  const atCore = useRef(0);

  /** Replaces one item's view, by id, in the list now shown. */
  const setItem = useCallback((view: ItemView) => {
    setList((now) => (now.state === "ready" ? { ...now, items: now.items.map((v) => (v.item.id === view.item.id ? view : v)) } : now));
  }, []);

  /** Sends the next inspects while there is room, each answer counted only for the read it belongs to (R19). */
  const pump = useCallback(() => {
    while (atCore.current < INSPECT_AT_ONCE && waiting.current.items.length > 0) {
      const item = waiting.current.items.shift()!;
      const mine = waiting.current.read;
      atCore.current += 1;
      void client
        .call("cleanup.inspect", { id: item.id })
        .then(
          (inspected): ItemView => ({ state: "inspected", item, inspected }),
          (error): ItemView => ({ state: "uninspectable", item, message: describe(error) }),
        )
        .then((view) => {
          atCore.current -= 1;
          if (mine === reading.current) setItem(view);
          pump();
        });
    }
  }, [client, describe, setItem]);

  const read = useCallback(
    (inactive: CleanUpDays) => {
      const mine = ++reading.current;
      setList({ state: "loading" });
      setChoices({});
      waiting.current = { read: mine, items: [] };
      client.call("cleanup.list", { inactive_days: inactive }).then(
        (answer) => {
          if (mine !== reading.current) return;
          setList({ state: "ready", items: answer.items.map((item) => ({ state: "checking", item })), unread: answer.unread });
          // The items are inspected a few at a time, in list order; an older read's waiting items are dropped.
          waiting.current = { read: mine, items: [...answer.items] };
          pump();
        },
        (error) => {
          if (mine === reading.current) setList({ state: "error", message: describe(error) });
        },
      );
    },
    [client, describe, pump],
  );

  const ticked = useMemo(() => {
    const on = new Set<string>();
    if (list.state === "ready") {
      for (const v of list.items) {
        // Only a checked item can be ticked: removing one needs the token its inspect gave.
        if (v.state === "inspected" && (choices[v.item.id] ?? tickedByDefault(v))) on.add(v.item.id);
      }
    }
    return on;
  }, [list, choices]);

  const choose = useCallback(
    (inactive: CleanUpDays) => {
      setDays(inactive);
      setResult(undefined);
      read(inactive);
    },
    [read],
  );

  const tick = useCallback((id: string, on: boolean) => setChoices((now) => ({ ...now, [id]: on })), []);

  const ask = useCallback(() => {
    const now = tickedOf(list, ticked);
    if (now.length > 0) setRun({ state: "asking", ticked: now });
  }, [list, ticked]);

  const cancel = useCallback(() => setRun((now) => (now?.state === "asking" ? undefined : now)), []);

  const stop = useCallback(() => {
    stopping.current = true;
    setRun((now) => (now?.state === "removing" ? { ...now, stopping: true } : now));
  }, []);

  const confirm = useCallback(() => {
    if (run?.state !== "asking") return;
    const items = run.ticked;
    stopping.current = false;
    setRun({ state: "removing", ticked: items, done: 0, stopping: false });
    setResult(undefined);
    void (async () => {
      const failures: Result["failures"] = [];
      let removed = 0;
      let done = 0;
      // One at a time, in list order; Stop lets the one under way finish and begins no other (R18).
      for (const inspected of items) {
        if (stopping.current) break;
        const item: CleanupItem = inspected;
        try {
          await client.call("cleanup.remove", { id: inspected.id, token: inspected.token, ...(sendsDiscard(inspected) ? { discard: true } : {}) });
          removed += 1;
        } catch (error) {
          // Changed since it was checked: nothing of it went. It shows as it is now, ticked again only if that
          // loses nothing, and the run goes on to the next (FLOW-CLEAN-UP, Variations).
          const refusal = refusalOf(error);
          const now = refusal?.kind === "invalid_state" ? changedTo(refusal.detail, inspected.id) : undefined;
          if (now) {
            setItem({ state: "inspected", item: now, inspected: now });
            setChoices(({ [inspected.id]: _, ...rest }) => rest);
            failures.push({ item, message: t("settings.storage.cleanUp.changedReason") });
          } else {
            failures.push({ item, message: describe(error) });
          }
        }
        done += 1;
        setRun((was) => (was?.state === "removing" ? { ...was, done } : was));
      }
      setRun(undefined);
      setResult({ removed, count: items.length, stopped: done < items.length, failures });
      // The list is read again when it ends, so what was removed leaves it and what is left shows as it is (R18).
      read(days);
    })();
  }, [run, client, describe, days, read, setItem]);

  const view: CleanUpView = { days, list, ticked, run, result };
  const actions: CleanUpActions = { choose, tick, ask, cancel, confirm, stop };
  return { view, actions, read: () => read(days) };
}
