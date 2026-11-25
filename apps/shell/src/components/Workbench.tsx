import { X } from "lucide-react";
import { type CSSProperties, type KeyboardEvent, type PointerEvent, type ReactNode, useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { t } from "../catalog";
import { type WidthLimits, clampWidth, limitsOf } from "../session/workbench";
import { milliseconds, reducedMotion } from "../theme/motion";
import { IconButton } from "./Icon";
import "./Workbench.css";

/** A token's value on `el`, in pixels or, for a percentage, as a share of one; NaN when it can't be read. */
function tokenOf(el: Element, name: string): number {
  const value = getComputedStyle(el).getPropertyValue(name).trim();
  return value.endsWith("%") ? parseFloat(value) / 100 : parseFloat(value);
}

/**
 * The workbench's limits where it is drawn (FLOW-READ-RAW-LOG-R16): the width it shares with the transcript area is
 * its parent's, the region beside the rail, and the least width and greatest share are the design's tokens.
 */
function limitsHere(aside: HTMLElement): WidthLimits {
  const shared = aside.parentElement?.getBoundingClientRect().width ?? 0;
  return limitsOf(shared, tokenOf(aside, "--size-workbench-min") || 0, tokenOf(aside, "--size-workbench-max") || 0);
}

/** The width the workbench is drawn at: the one the user chose, or the design's, within its limits now. */
function shownWidth(aside: HTMLElement, width: number | undefined): { width: number; limits: WidthLimits } {
  const limits = limitsHere(aside);
  return { width: clampWidth(width ?? (tokenOf(aside, "--size-workbench") || 0), limits), limits };
}

/**
 * The workbench (LAY-SHELL, The workbench; docs/design/WORKBENCH.md): a region at the window's right, beside the
 * transcript, holding the developer pane. Its width grows from nothing as it opens and runs back as it closes, while
 * the transcript area beside it narrows and widens with it; at once with reduced motion on, or when `eases` is false,
 * as when the window starts with it kept open. Its leading edge resizes it, by pointer or keyboard.
 */
export function Workbench({
  open,
  width,
  eases = true,
  inert,
  onResize,
  onClose,
  focusAfterClose,
  force,
  children,
}: {
  open: boolean;
  /** The width the user chose, in CSS pixels; undefined is the design's. The drawn width keeps within its limits. */
  width: number | undefined;
  /** Whether opening and closing move; false shows and removes it at once. */
  eases?: boolean;
  /** Out of reach, as behind the delete question. */
  inert?: boolean;
  /** A width the user chose by the edge; `keep` once a drag ends or a key has moved it, so it can be kept. */
  onResize: (width: number, keep: boolean) => void;
  /** The pane's Close. */
  onClose: () => void;
  /** Where keyboard focus goes when the workbench closes with focus inside it: the arrange button (R01). */
  focusAfterClose: () => HTMLElement | null | undefined;
  /** For the gallery: the resize edge under the pointer, being dragged, or with keyboard focus. */
  force?: "hover" | "pressed" | "focus";
  /** The pane's body, beneath its head. */
  children?: ReactNode;
}) {
  const aside = useRef<HTMLElement>(null);
  const pane = useRef<HTMLDivElement>(null);
  const head = useId();
  // Drawn while open, and while it runs back after closing.
  const [shown, setShown] = useState(open);
  if (open && !shown) setShown(true);
  const running = useRef<Animation | undefined>(undefined);
  // The limits and the drawn width, for assistive technology, measured as it opens and as the edge moves.
  const [measured, setMeasured] = useState<{ width: number; limits: WidthLimits }>();

  useLayoutEffect(() => {
    const el = aside.current;
    if (!el || !shown) return;
    // Measured with no motion running, so the width is the one it rests at.
    const from = running.current ? el.getBoundingClientRect().width : open ? 0 : el.getBoundingClientRect().width;
    running.current?.cancel();
    running.current = undefined;
    if (pane.current) pane.current.style.minWidth = "";
    if (!open) {
      // Focus inside never falls to the window: it goes to the arrange button before the workbench is out of reach.
      if (el.contains(document.activeElement)) focusAfterClose()?.focus({ preventScroll: true });
    } else setMeasured(shownWidth(el, width));
    const to = open ? el.getBoundingClientRect().width : 0;
    const style = getComputedStyle(el);
    const duration = milliseconds(style.getPropertyValue("--motion-settle"));
    const easing = style.getPropertyValue("--motion-ease-out").trim();
    if (!eases || reducedMotion() || typeof el.animate !== "function" || !duration || !easing || from === to) {
      if (!open) setShown(false);
      return;
    }
    // The pane keeps its full width while the region grows or shrinks over it, so its head doesn't reflow.
    if (pane.current) pane.current.style.minWidth = `${Math.max(from, to)}px`;
    const animation = el.animate([{ width: `${from}px` }, { width: `${to}px` }], { duration, easing, fill: open ? "none" : "forwards" });
    running.current = animation;
    animation.onfinish = () => {
      if (running.current !== animation) return;
      running.current = undefined;
      if (pane.current) pane.current.style.minWidth = "";
      if (!open) setShown(false);
    };
    // Runs as it opens or closes; a change of width while open is the edge's, which moves with no easing.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, shown]);

  // The window resizing moves the drawn width and its limits, through the stylesheet's clamp: measured again, so
  // assistive technology hears what is drawn.
  useEffect(() => {
    if (!open) return;
    const remeasure = () => aside.current && setMeasured(shownWidth(aside.current, width));
    window.addEventListener("resize", remeasure);
    return () => window.removeEventListener("resize", remeasure);
  }, [open, width]);

  // The width the user chose, within its limits, from where it is drawn now: a kept width wider than the window
  // allows starts from what is shown, so the first step moves it.
  const resize = useCallback(
    (to: (now: number, limits: WidthLimits) => number, keep: boolean) => {
      const el = aside.current;
      if (!el) return;
      const now = shownWidth(el, width);
      const next = clampWidth(to(now.width, now.limits), now.limits);
      setMeasured({ width: next, limits: now.limits });
      onResize(next, keep);
    },
    [width, onResize],
  );

  // A drag follows the pointer with no easing: the edge moves by as much as the pointer, and the width with it.
  const drag = useRef<{ x: number; width: number; moved: boolean } | undefined>(undefined);
  const [dragging, setDragging] = useState(false);
  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    const el = aside.current;
    if (event.button !== 0 || !el) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    drag.current = { x: event.clientX, width: shownWidth(el, width).width, moved: false };
    setDragging(true);
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const from = drag.current;
    if (!from) return;
    from.moved = true;
    // The workbench is at the window's right, so the edge moving left widens it.
    resize(() => from.width + from.x - event.clientX, false);
  };
  const endDrag = () => {
    const from = drag.current;
    drag.current = undefined;
    setDragging(false);
    // Kept once, as the drag ends.
    if (from?.moved) resize((now) => now, true);
  };

  // Left and Right move the edge by space-lg the way the arrow points; Home and End to the least and greatest width.
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const el = aside.current;
    if (!el) return;
    const step = tokenOf(el, "--space-lg") || 0;
    const to: Record<string, (now: number, limits: WidthLimits) => number> = {
      ArrowLeft: (now) => now + step,
      ArrowRight: (now) => now - step,
      Home: (_, limits) => limits.least,
      End: (_, limits) => limits.most,
    };
    const move = to[event.key];
    if (!move) return;
    event.preventDefault();
    resize(move, true);
  };

  if (!shown) return null;
  const closing = !open;
  return (
    <aside
      ref={aside}
      className="workbench"
      aria-labelledby={head}
      // Out of reach while it runs back, and hidden from assistive technology.
      inert={closing || inert}
      aria-hidden={closing || undefined}
      data-dragging={dragging || force === "pressed" || undefined}
      style={width === undefined ? undefined : ({ "--workbench-width": `${width}px` } as CSSProperties)}
    >
      <div
        className="workbench-edge"
        role="separator"
        aria-orientation="vertical"
        aria-label={t("workbench.resizeLabel")}
        aria-valuenow={measured?.width}
        aria-valuemin={measured?.limits.least}
        aria-valuemax={measured?.limits.most}
        tabIndex={0}
        // Dragged, the pointer is on the edge too, as it is in a real drag.
        data-force={force === "pressed" ? "hover" : force}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onKeyDown={onKeyDown}
      />
      <div ref={pane} className="workbench-pane">
        <div className="workbench-head">
          <h2 id={head} className="workbench-name">
            {t("developer.nameHeading")}
          </h2>
          <IconButton icon={X} size="icon-sm" label={t("workbench.closeButton")} className="workbench-close" onClick={onClose} />
        </div>
        {children}
      </div>
    </aside>
  );
}
