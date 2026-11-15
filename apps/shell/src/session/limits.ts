// The limits the session controls' ring reads (LAY-SHELL, Context; docs/design/COMPOSER.md, The context ring): the
// session's context and each usage window its backend reported. Kept free of React so the tests read it directly.

import { t } from "../catalog";
import type { UsageUpdated } from "../protocol/agent-events.gen";

/** A usage window (GLOSSARY) as `usage.updated` reports it. */
export type UsageWindow = NonNullable<NonNullable<UsageUpdated["rate_limit"]>["windows"]>[number];

/** One reading the ring can show: the context, which has no label, or a usage window by its label. */
export interface Reading {
  /** The window's name; absent for the context. */
  label?: string;
  /** The whole percent the ring shows; absent while the context isn't reported. */
  percent?: number;
  /** A context share above nothing but under 1 percent, which reads as less than 1 percent, never as none. */
  under1?: true;
  /** When the window resets, as the backend said it (RFC 3339). */
  resetsAt?: string;
}

/** The context's reading, as the ring has always read it: never more than full, and a share Core couldn't measure reads as none. */
export function contextReading(used: number | undefined): Reading {
  if (used === undefined || !Number.isFinite(used)) return {};
  const share = Math.min(Math.max(used, 0), 1);
  return { percent: Math.round(share * 100), ...(share > 0 && share < 0.01 ? { under1: true as const } : {}) };
}

/**
 * Every reading, one to a line and never re-sorted: the context first, then each window in the order the backend
 * reported. A window's share is the whole percent, rounded down, never more than full. A window Core passed on that
 * isn't well formed (no label, or a share that isn't a number) is left out rather than shown wrong.
 */
export function readings(context: number | undefined, windows: readonly UsageWindow[] | undefined): Reading[] {
  const shown = (windows ?? []).filter((w) => typeof w.label === "string" && Number.isFinite(w.used));
  return [
    contextReading(context),
    ...shown.map((w) => ({
      label: w.label,
      // A hair added before rounding down, since 0.29 * 100 is 28.999… in floating point and must read 29.
      percent: Math.min(Math.max(Math.floor(w.used * 100 + 1e-9), 0), 100),
      ...(typeof w.resets_at === "string" ? { resetsAt: w.resets_at } : {}),
    })),
  ];
}

/**
 * The nearest limit: the reading with the largest figure, compared as shown so the ring never reads less than a line
 * of its own list. The first wins a tie, so the context does; a context not reported yet loses to any window.
 */
export function nearest(list: readonly Reading[]): Reading {
  return list.reduce((best, r) => (r.percent !== undefined && (best.percent === undefined || r.percent > best.percent) ? r : best));
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
// A weekday and time in the locale the catalog is in, in the user's own time zone, as Claude Desktop's list reads.
const weekdayTime = new Intl.DateTimeFormat("en-US", { weekday: "short", hour: "numeric", minute: "2-digit" });

/**
 * When a window resets, as Claude Desktop's plan usage limits word it: a weekday and time a day or more away ("Resets
 * Wed 2:00 PM"), else the time left ("Resets in 1 hr 15 min", "Resets in 2 hr", "Resets in 15 min"). Minutes round up,
 * so the words never say a limit has reset before it has; a time already past reads as a minute away, since the
 * backend hasn't reported the fresh window yet. Nothing for a time that can't be read.
 */
export function resetsIn(resetsAt: string | undefined, now: Date): string | undefined {
  if (resetsAt === undefined) return undefined;
  const at = new Date(resetsAt);
  if (Number.isNaN(at.getTime())) return undefined;
  const left = at.getTime() - now.getTime();
  if (left <= 0) return undefined;
  if (left >= DAY) {
    // Intl joins the weekday with a comma; Claude Desktop's list doesn't.
    return t("sessionControls.contextRing.resetsAtLabel", { when: weekdayTime.format(at).replace(",", "") });
  }
  const minutes = Math.ceil(left / MINUTE);
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) return t("sessionControls.contextRing.resetsInMinutesLabel", { minutes: rest });
  if (rest === 0) return t("sessionControls.contextRing.resetsInHoursLabel", { hours });
  return t("sessionControls.contextRing.resetsInHoursMinutesLabel", { hours, minutes: rest });
}
