// The workbench's open state and width (FLOW-READ-RAW-LOG, step 1; R01, R02, R16). Core keeps both in the user's
// settings under `workbench` (PROTO-CORE-API, Settings), without checking them, so the window reads them defensively
// and sends each change as a merge patch. Kept for the window, not for each session.

import { useCallback, useRef, useState } from "react";
import { objectOr } from "./arrangement";

/** What the window keeps of the workbench: whether it is open, and the width the user chose; undefined is the design's. */
interface WorkbenchState {
  open: boolean;
  width: number | undefined;
}

/** Closed and at the design's width: what a window with nothing kept starts with (R02). */
const CLOSED: WorkbenchState = { open: false, width: undefined };

/** A JSON merge patch (RFC 7386) for the workbench, as `settings.set` takes it. */
export type WorkbenchPatch = { workbench: { open: boolean } | { width: number } };

/**
 * The workbench as kept in `settings`. Settings are the user's and may hold anything: it is open only when `open` is
 * exactly true, and a width is kept only when it is a positive number.
 */
function workbenchOf(settings: unknown): WorkbenchState {
  const kept = objectOr(objectOr(settings)?.workbench);
  const width = kept?.width;
  return { open: kept?.open === true, width: typeof width === "number" && Number.isFinite(width) && width > 0 ? width : undefined };
}

/** The workbench's limits (R16): the most is `maxShare` of the width it shares with the transcript area, and the least is `min` while that allows it. */
export interface WidthLimits {
  least: number;
  most: number;
}

/** The limits in a shared width of `shared` pixels, from the design's least width and greatest share. */
export function limitsOf(shared: number, min: number, maxShare: number): WidthLimits {
  const most = Math.max(0, shared * maxShare);
  return { least: Math.min(min, most), most };
}

/** `width` kept within `limits`, rounded to whole CSS pixels. */
export function clampWidth(width: number, { least, most }: WidthLimits): number {
  return Math.round(Math.min(Math.max(width, least), most));
}

/**
 * The workbench as the window shows it. Every change shows at once and is then sent to Core; one Core refuses or never
 * hears is left as the user set it, and nothing is said (R02). Core's settings are applied once, from the first read
 * that succeeds, and only while the user has not yet opened, closed or resized the workbench in this window: a later
 * read, on reconnecting, would otherwise undo what a refused write left in place.
 */
export function useWorkbench(save: (patch: WorkbenchPatch) => Promise<unknown>) {
  const [state, setState] = useState<WorkbenchState>(CLOSED);
  // Set once Core's settings are applied, or once the user has changed the workbench, whichever is first.
  const settled = useRef(false);
  // Whether it opens and closes with motion: only once the user has opened or closed it here, so a window that starts
  // with it kept open shows it at once.
  const [eases, setEases] = useState(false);

  /** Core's settings, read on a connection. */
  const read = useCallback((settings: unknown) => {
    if (settled.current) return;
    settled.current = true;
    setState(workbenchOf(settings));
  }, []);

  const send = useCallback(
    (patch: WorkbenchPatch) => {
      settled.current = true;
      // Best effort: a refused write leaves the workbench as the user set it, in this window alone.
      save(patch).catch(() => {});
    },
    [save],
  );

  const setOpen = useCallback(
    (open: boolean) => {
      setState((s) => ({ ...s, open }));
      setEases(true);
      send({ workbench: { open } });
    },
    [send],
  );

  /** A width the user chose, shown at once; `keep` sends it to Core too, as a drag ends or a key moves the edge. */
  const resize = useCallback(
    (width: number, keep: boolean) => {
      setState((s) => ({ ...s, width }));
      if (keep) send({ workbench: { width } });
      else settled.current = true;
    },
    [send],
  );

  return { ...state, eases, read, setOpen, resize };
}
