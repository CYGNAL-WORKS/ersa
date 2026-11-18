// The user's sections (FLOW-ORGANIZE-SESSIONS, step 5; PROTO-CORE-API, Sections): read from Core on each new
// connection, and changed through Core. Core announces no change to them, so the list takes each change from Core's
// answer to it. Which section a session is in is the session's own, in `session.list`.

import { useCallback, useEffect, useState } from "react";
import { t } from "../catalog";
import type { CoreClient } from "../core/client";
import type { Section } from "../protocol/core-api.gen";
import type { Sending } from "./follow";

export type SectionsView = { state: "loading" } | { state: "ready"; sections: Section[] } | { state: "error"; message: string };

/** A change to a session's place on its way to Core, or why Core refused the last: a pin or a move to a section. */
export type Moving = Sending & { change: "pin" | "section" };

/** Why nothing can be put in a section now: the sections are still being read, or can't be (FLOW-ORGANIZE-SESSIONS-R34). */
export function sectionsUnavailable(sections: SectionsView | undefined): string | undefined {
  if (sections?.state === "loading") return t("rail.tree.loadingProgress");
  if (sections?.state === "error") return t("rail.tree.sectionsError");
  return undefined;
}

/**
 * The sections and the changes to them on their way. `link` counts connections: a new one reads the list again.
 * `sessionsChanged` is called once Core has moved sessions, so the caller reads `session.list` again.
 */
export function useSections(client: CoreClient, link: number | undefined, describe: (error: unknown) => string, sessionsChanged: () => void) {
  const [view, setView] = useState<SectionsView>({ state: "loading" });
  const [renaming, setRenaming] = useState<Record<string, Sending>>({});
  const [removing, setRemoving] = useState<Record<string, Sending>>({});
  // A new section on its way, or why Core refused the last (FLOW-ORGANIZE-SESSIONS, Variations).
  const [creating, setCreating] = useState<Sending>();

  useEffect(() => {
    if (link === undefined) return;
    let live = true;
    client.call("section.list").then(
      (sections) => live && setView({ state: "ready", sections }),
      // A failed reread keeps the list the rail already shows.
      (error) => live && setView((v) => (v.state === "ready" ? v : { state: "error", message: describe(error) })),
    );
    return () => {
      live = false;
    };
  }, [client, link, describe]);

  // Core's answer replaces the section in the list, so the name shown is the one Core keeps, its spacing settled.
  const put = (section: Section) =>
    setView((v) => (v.state === "ready" ? { state: "ready", sections: v.sections.map((s) => (s.id === section.id ? section : s)) } : v));

  /**
   * Makes a section, resolving with the section Core answers with, or undefined once Core refused it, its reason kept
   * in `creating`. A name exactly another section's gives that section, and nothing new appears (R23): Core compares
   * names, case and all, and the window only takes its answer. A new one goes last, as Core lists them in the order
   * they were made (R04).
   */
  const create = async (name: string): Promise<Section | undefined> => {
    setCreating({ state: "sending" });
    try {
      const section = await client.call("section.create", { name });
      setCreating(undefined);
      setView((v) => (v.state === "ready" && !v.sections.some((s) => s.id === section.id) ? { state: "ready", sections: [...v.sections, section] } : v));
      return section;
    } catch (error) {
      setCreating({ state: "failed", message: describe(error) });
      return undefined;
    }
  };
  const createReset = useCallback(() => setCreating(undefined), []);

  /** Renames a section; resolves true once Core has, else keeps Core's reason in `renaming` (FLOW-ORGANIZE-SESSIONS, Variations). */
  const rename = async (id: string, name: string): Promise<boolean> => {
    setRenaming((r) => ({ ...r, [id]: { state: "sending" } }));
    try {
      put(await client.call("section.rename", { id, name }));
      setRenaming(({ [id]: _, ...rest }) => rest);
      return true;
    } catch (error) {
      setRenaming((r) => ({ ...r, [id]: { state: "failed", message: describe(error) } }));
      return false;
    }
  };
  const renameReset = useCallback((id: string) => setRenaming(({ [id]: _, ...rest }) => rest), []);

  /**
   * Deletes a section, asking nothing: Core puts its sessions in no section and keeps every one (R05). Once it has,
   * the heading goes and the sessions are read again, which shows them under "No section".
   */
  const remove = async (id: string) => {
    setRemoving((r) => ({ ...r, [id]: { state: "sending" } }));
    try {
      await client.call("section.remove", { id });
      setRemoving(({ [id]: _, ...rest }) => rest);
      setView((v) => (v.state === "ready" ? { state: "ready", sections: v.sections.filter((s) => s.id !== id) } : v));
      sessionsChanged();
    } catch (error) {
      setRemoving((r) => ({ ...r, [id]: { state: "failed", message: describe(error) } }));
    }
  };

  return { view, creating, create, createReset, renaming, renameReset, rename, removing, remove };
}
