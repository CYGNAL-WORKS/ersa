// What the sessions page lists and how it says when a session was last active (FLOW-ORGANIZE-SESSIONS, step 9;
// docs/design/RAIL.md, The sessions page). Free of React, and of the rail's arrangement: the page lists every session
// whatever the rail's grouping, filter or collapsed groups.

import { t } from "../catalog";
import type { Session } from "../protocol/core-api.gen";

export type SessionsTab = "active" | "archived";

type Listed = Pick<Session, "archived" | "title" | "created_at" | "last_activity_at">;

/** A session's time on the page: its last activity, else its creation. */
const timeOf = (s: Pick<Session, "created_at" | "last_activity_at">) => s.last_activity_at ?? s.created_at;

/** Milliseconds since the epoch, so the two kinds of timestamp compare whatever their precision; 0 if unreadable. */
const millis = (iso: string) => Date.parse(iso) || 0;

/**
 * The rows for a tab and the search text (R16, R17): newest first by last activity, taking creation for it until the
 * session has one, and only those whose title, as the page shows it, contains the text, ignoring case.
 */
export function pageRows<S extends Listed>(sessions: readonly S[], tab: SessionsTab, search: string): S[] {
  const wanted = search.toLowerCase();
  return sessions
    .filter((s) => s.archived === (tab === "archived"))
    .filter((s) => (s.title || t("rail.tile.untitledLabel")).toLowerCase().includes(wanted))
    .sort((a, b) => millis(timeOf(b)) - millis(timeOf(a)));
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
// The date reads in the locale the catalog is in, in the user's own time zone.
const date = new Intl.DateTimeFormat("en-US", { dateStyle: "medium" });

/** The local day `date` falls on, as a whole number, so two days' difference counts calendar days whatever the hour. */
const dayOf = (d: Date) => Math.round(new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() / (24 * HOUR));

/** When a row's session was last active, as the page words it: "Just now", "5 minutes ago", "Yesterday", then a date. */
export function lastActive(session: Pick<Session, "created_at" | "last_activity_at">, now: Date): string {
  const when = new Date(timeOf(session));
  if (Number.isNaN(when.getTime())) return "";
  const since = now.getTime() - when.getTime();
  // A time ahead of this machine's clock reads as now.
  if (since < MINUTE) return t("rail.sessionsPage.justNowLabel");
  if (since < HOUR) return t("rail.sessionsPage.minutesAgoLabel", { count: Math.floor(since / MINUTE) });
  const days = dayOf(now) - dayOf(when);
  if (days === 0) return t("rail.sessionsPage.hoursAgoLabel", { count: Math.floor(since / HOUR) });
  if (days === 1) return t("rail.sessionsPage.yesterdayLabel");
  return date.format(when);
}
