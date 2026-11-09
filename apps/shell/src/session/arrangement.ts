// How the user has the rail arranged (FLOW-ORGANIZE-SESSIONS, steps 4, 6 and 7): its grouping, its sort, what it shows, and
// the groups collapsed in each grouping. Core keeps it in the user's settings under `rail` (PROTO-CORE-API,
// Settings), so it survives a restart of Ersa and of Core (R13); the window only reads it and sends changes as merge
// patches. Pins and sections are the sessions' own and live in Core's records, not here.

import { useCallback, useEffect, useRef, useState } from "react";

/** How the rail groups the sessions after the Pinned group. Grouping by section comes with sections. */
export type GroupBy = "project" | "date" | "none";
/** How the tiles are sorted within each group (FLOW-ORGANIZE-SESSIONS-R09), named as `rail.sort_by` names it. */
export type SortBy = "created" | "last_activity" | "name";
/** Which sessions the rail shows (FLOW-ORGANIZE-SESSIONS-R11). */
export type Show = "active" | "archived" | "all";

export const GROUP_BYS: readonly GroupBy[] = ["project", "date", "none"];
export const SORT_BYS: readonly SortBy[] = ["created", "last_activity", "name"];
export const SHOWS: readonly Show[] = ["active", "archived", "all"];

export interface RailArrangement {
  groupBy: GroupBy;
  sortBy: SortBy;
  show: Show;
  /**
   * The keys of the groups collapsed in each grouping (R12): `pinned`, a project's id, or a date group's key. A
   * grouping with none collapsed may be absent.
   */
  collapsed: Partial<Record<GroupBy, readonly string[]>>;
}

/** The choices the arrange menu names: the arrangement but for what is collapsed. */
export type ArrangeChoices = Pick<RailArrangement, "groupBy" | "sortBy" | "show">;

/** What Reset to defaults returns to (R14), and what settings that hold nothing of the rail read as. */
export const DEFAULT_ARRANGEMENT: RailArrangement = { groupBy: "project", sortBy: "created", show: "active", collapsed: {} };

/** A JSON merge patch (RFC 7386), as `settings.set` takes it. */
export type Patch = { rail: Record<string, unknown> };

/** The patch that chooses a grouping, a sort or a filter. */
export function choicePatch(choice: { groupBy: GroupBy } | { sortBy: SortBy } | { show: Show }): Patch {
  if ("groupBy" in choice) return { rail: { group_by: choice.groupBy } };
  if ("sortBy" in choice) return { rail: { sort_by: choice.sortBy } };
  return { rail: { show: choice.show } };
}

/** The patch that keeps `keys` as the groups collapsed in `groupBy`; an array replaces the one Core has. */
export function collapsePatch(groupBy: GroupBy, keys: readonly string[]): Patch {
  return { rail: { collapsed: { [groupBy]: [...keys] } } };
}

/**
 * Reset to defaults (R14): each `rail.*` key sent as null, which removes it, so each reads as its default. Pins and
 * sections are not settings, so they stay.
 */
export const RESET_PATCH: Patch = { rail: { group_by: null, sort_by: null, show: null, collapsed: null } };

/** The rail's arrangement in `settings`. Settings are the user's and may hold anything: what does not read is the default. */
export function arrangementOf(settings: unknown): RailArrangement {
  const rail = objectOr(objectOr(settings)?.rail);
  const groupBy = GROUP_BYS.find((g) => g === rail?.group_by) ?? DEFAULT_ARRANGEMENT.groupBy;
  const sortBy = SORT_BYS.find((s) => s === rail?.sort_by) ?? DEFAULT_ARRANGEMENT.sortBy;
  const show = SHOWS.find((s) => s === rail?.show) ?? DEFAULT_ARRANGEMENT.show;
  const stored = objectOr(rail?.collapsed);
  const collapsed: RailArrangement["collapsed"] = {};
  for (const g of GROUP_BYS) {
    const keys = stored?.[g];
    if (Array.isArray(keys)) collapsed[g] = keys.filter((k): k is string => typeof k === "string");
  }
  return { groupBy, sortBy, show, collapsed };
}

/** `target` with `patch` merged in, as RFC 7386 gives: objects merge key by key, null removes, anything else replaces. */
export function mergePatch(target: unknown, patch: unknown): unknown {
  const changes = objectOr(patch);
  if (!changes) return patch;
  const out: Record<string, unknown> = { ...objectOr(target) };
  for (const [key, value] of Object.entries(changes)) {
    if (value === null) delete out[key];
    else out[key] = mergePatch(out[key], value);
  }
  return out;
}

function objectOr(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined;
}

/** A problem with the arrangement the rail says: it could not be read, or a change could not be saved. */
export interface ArrangeProblem {
  kind: "read" | "save";
  message: string;
}

/**
 * The rail's arrangement as the window shows it: Core's settings as last read, with each change Core has saved since
 * and each still on its way merged in, so the rail changes at once. A change Core refuses is dropped, so the rail
 * shows what Core has, and the rail says why until the next change (FLOW-ORGANIZE-SESSIONS, Saving fails).
 */
export function useRailArrangement(save: (patch: Patch) => Promise<unknown>, describe: (error: unknown) => string) {
  // Core's settings, as last read and with every saved change merged in; undefined until first read.
  const [stored, setStored] = useState<{ settings: unknown }>();
  const [pending, setPending] = useState<{ id: number; patch: Patch }[]>([]);
  const [problem, setProblem] = useState<ArrangeProblem>();
  const nextId = useRef(0);
  const arrangement = arrangementOf(pending.reduce((settings, p) => mergePatch(settings, p.patch), stored?.settings));
  const change = useCallback(
    (patch: Patch) => {
      const id = nextId.current++;
      setPending((p) => [...p, { id, patch }]);
      setProblem(undefined);
      const settle = () => setPending((p) => p.filter((c) => c.id !== id));
      save(patch).then(
        () => {
          setStored((s) => ({ settings: mergePatch(s?.settings, patch) }));
          settle();
        },
        (error) => {
          setProblem({ kind: "save", message: describe(error) });
          settle();
        },
      );
    },
    [save, describe],
  );
  /** Core's settings, read on each connection. */
  const read = useCallback((settings: unknown) => {
    setStored({ settings });
    // Read now, so an earlier failure to read no longer holds; a refused change still does.
    setProblem((p) => (p?.kind === "read" ? undefined : p));
  }, []);
  /** They could not be read: the rail is arranged by the defaults and says why. */
  const readFailed = useCallback((message: string) => {
    setStored((s) => s ?? { settings: {} });
    setProblem({ kind: "read", message });
  }, []);
  return { arrangement, reading: stored === undefined, problem, change, read, readFailed };
}

/** Now, taken again just after each local midnight: what the date groups count from (FLOW-ORGANIZE-SESSIONS-R07). */
export function useLocalDay(): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const next = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
    // A second past midnight, so a timer that fires a little early still lands on the new day.
    const timer = setTimeout(() => setNow(new Date()), next.getTime() - Date.now() + 1000);
    return () => clearTimeout(timer);
  }, [now]);
  return now;
}
