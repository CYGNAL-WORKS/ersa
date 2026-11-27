// Archive inactive sessions, Storage's setting (FLOW-CLEAN-UP, step 4; R15). Core keeps it in the user's settings as
// `sessions.archive_inactive_days` and refuses any value but these (PROTO-CORE-API-R126); unset is Never. The window
// reads it from the settings it reads on each connection and sends a change as a merge patch.

import { useCallback, useState } from "react";
import { objectOr } from "./arrangement";
import type { Sending } from "./follow";

/** The days the setting can take, after Never. */
export const ARCHIVE_DAYS = [1, 2, 7, 14, 30] as const;
export type ArchiveDays = (typeof ARCHIVE_DAYS)[number];

/**
 * The value in force in `settings`, as `settings.get` returns them: a number of days, or undefined for Never. Settings
 * are the user's and may hold anything, so a value Core would refuse reads as Never, which is what the sweep does
 * with it.
 */
export function archiveDaysOf(settings: unknown): ArchiveDays | undefined {
  const days = objectOr(objectOr(settings)?.sessions)?.archive_inactive_days;
  return ARCHIVE_DAYS.find((d) => d === days);
}

/** A JSON merge patch (RFC 7386) for `settings.set`: the choice, or null for Never, which clears the key. */
export type ArchivePatch = { sessions: { archive_inactive_days: ArchiveDays | null } };
export function archivePatch(days: ArchiveDays | undefined): ArchivePatch {
  return { sessions: { archive_inactive_days: days ?? null } };
}

/** The setting as the window shows it: still being read, read, or not readable and why. */
export type ArchiveView = { state: "loading" } | { state: "ready"; value: ArchiveDays | undefined } | { state: "error"; message: string };

/**
 * Archive inactive sessions in the window. Core's settings set it on each connection's read; a choice shows at once
 * while it is sent, and one Core refuses goes back to the value in force, with the reason in `changing`
 * (docs/design/SETTINGS.md, A setting's states).
 */
export function useArchiveSetting(save: (patch: ArchivePatch) => Promise<unknown>, describe: (error: unknown) => string) {
  const [view, setView] = useState<ArchiveView>({ state: "loading" });
  const [changing, setChanging] = useState<Sending>();

  /** Core's settings, read on a connection. */
  const read = useCallback((settings: unknown) => setView({ state: "ready", value: archiveDaysOf(settings) }), []);
  const unread = useCallback((error: unknown) => setView({ state: "error", message: describe(error) }), [describe]);

  const choose = useCallback(
    (days: ArchiveDays | undefined) => {
      if (view.state !== "ready") return;
      const inForce = view.value;
      setView({ state: "ready", value: days });
      setChanging({ state: "sending" });
      save(archivePatch(days)).then(
        () => setChanging(undefined),
        (error) => {
          setView((now) => (now.state === "ready" ? { state: "ready", value: inForce } : now));
          setChanging({ state: "failed", message: describe(error) });
        },
      );
    },
    [view, save, describe],
  );

  return { view, changing, read, unread, choose };
}
