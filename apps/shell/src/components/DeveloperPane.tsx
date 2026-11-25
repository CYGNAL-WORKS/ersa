import { Funnel } from "lucide-react";
import { type RefObject, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { t } from "../catalog";
import type { RawLogEntry } from "../protocol/core-api.gen";
import type { RawLogView } from "../session/rawLog";
import { lineMatcher, lineText } from "../session/rawLogText";
import { Icon } from "./Icon";
import { RawLogList } from "./RawLogList";
import "./DeveloperPane.css";

/**
 * The lines the filter leaves in (FLOW-READ-RAW-LOG-R12). Each line's answer is kept for the filter's text, so a read
 * that adds lines tests only the new ones; a new text starts again.
 */
function useShownLines(lines: RawLogEntry[] | undefined, filter: string): RawLogEntry[] | undefined {
  const answers = useRef<{ text: string; matches: (line: string) => boolean; known: WeakMap<RawLogEntry, boolean> }>(undefined);
  return useMemo(() => {
    if (lines === undefined || filter === "") return lines;
    if (answers.current?.text !== filter) answers.current = { text: filter, matches: lineMatcher(filter), known: new WeakMap() };
    const { matches, known } = answers.current;
    return lines.filter((line) => {
      let answer = known.get(line);
      if (answer === undefined) {
        answer = matches(lineText(line));
        known.set(line, answer);
      }
      return answer;
    });
  }, [lines, filter]);
}

/**
 * The developer pane's body beneath its head (FLOW-READ-RAW-LOG; docs/design/WORKBENCH.md, The developer pane): the
 * toolbar with the filter field and, while it holds text, how many of the lines held are listed; then the list, with
 * the detail beneath it. The filter stays when the active session changes (step 7); the list starts again for each
 * session.
 */
export function DeveloperPane({
  session,
  view,
  backend,
  attachedTo,
  onOpenDiagnosticLog,
  open = true,
  force,
}: {
  /** The active session, or undefined while the new-session screen shows. */
  session: string | undefined;
  /** What the reader holds of the active session's raw log. */
  view: RawLogView | undefined;
  backend: string;
  /** The labeled endpoint a debug build is attached to, where Ersa has no diagnostic log of its own (FLOW-READ-RAW-LOG-R13). */
  attachedTo?: string;
  /** Asks the Shell to open the diagnostic log; rejects with the reason when it couldn't. */
  onOpenDiagnosticLog?: () => Promise<void>;
  /** Whether the workbench is open; closing it ends what the pane says about the diagnostic log (R26). */
  open?: boolean;
  /** For the gallery: the filter's text, the field with keyboard focus, and the list's states. */
  force?: {
    filter?: string;
    field?: "focus";
    list?: Parameters<typeof RawLogList>[0]["force"];
    /** Open diagnostic log under the pointer, pressed or with keyboard focus. */
    openLog?: "hover" | "pressed" | "focus";
    /** Why the diagnostic log couldn't be opened, as after a choice of Open diagnostic log that failed. */
    unopened?: string;
    /** The toolbar in two rows, or one, whatever its width. */
    narrow?: boolean;
  };
}) {
  const [filter, setFilter] = useState(force?.filter ?? "");
  // Why the diagnostic log couldn't be opened, from the last choice of Open diagnostic log. It stands until the user's
  // next press or key in the pane, a change of active session, or the workbench closing (FLOW-READ-RAW-LOG-R26).
  const [unopened, setUnopened] = useState(force?.unopened);
  const [unopenedFor, setUnopenedFor] = useState({ session, open });
  if (unopenedFor.session !== session || unopenedFor.open !== open) {
    setUnopenedFor({ session, open });
    if (unopened !== undefined && (unopenedFor.session !== session || !open)) setUnopened(undefined);
  }
  const unavailable = attachedTo !== undefined ? t("developer.openLogUnavailableReason", { label: attachedTo }) : undefined;
  const why = useId();
  const openLog = () => {
    if (unavailable !== undefined || !onOpenDiagnosticLog) return;
    onOpenDiagnosticLog().then(
      () => setUnopened(undefined),
      (reason: unknown) => setUnopened(String(reason)),
    );
  };
  // Any press or key in the toolbar, the list or the detail ends the line; the press that opens the log again comes
  // first, so a choice that fails again says so again.
  const dismiss = () => {
    if (unopened !== undefined) setUnopened(undefined);
  };
  const toolbar = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const narrow = useNarrowToolbar(toolbar, button, force?.narrow);
  const shown = useShownLines(view?.lines, filter);
  const count = useId();
  const filtering = filter !== "";
  // The count keeps its words while it fades out after the field empties.
  const counted =
    filtering && view
      ? t("developer.filterCountDetail", { shown: (shown?.length ?? 0).toLocaleString("en-US"), total: view.lines.length.toLocaleString("en-US") })
      : undefined;
  const [said, setSaid] = useState(counted);
  if (counted !== undefined && counted !== said) setSaid(counted);

  return (
    // Laid out as if it weren't there (display: contents), so the toolbar and the list stay the pane's own rows.
    <div className="developer-pane" onPointerDownCapture={dismiss} onKeyDownCapture={dismiss}>
      <div ref={toolbar} className="developer-toolbar" data-narrow={narrow || undefined}>
        {/* The field and the count keep together and share the width the button leaves. */}
        <div className="developer-filter-group">
          <label className="developer-filter" data-force={force?.field}>
            <Icon icon={Funnel} size="icon-sm" />
            <input
              type="search"
              className="developer-filter-field"
              placeholder={t("developer.filterLabel")}
              aria-label={t("developer.filterLabel")}
              aria-describedby={counted !== undefined ? count : undefined}
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              onKeyDown={(event) => {
                // Escape empties the field; with nothing in it, Escape is left to what holds the pane.
                if (event.key !== "Escape" || filter === "") return;
                event.preventDefault();
                setFilter("");
              }}
            />
          </label>
          {/* Always in the page, so it fades in and out. Shown only while there is a count of this session's lines: none
              while the next session is read, or with no session, so it never counts the session left (R04, R12). */}
          <span id={count} className="developer-count" data-shown={counted !== undefined || undefined} aria-hidden={counted === undefined || undefined}>
            {said}
          </span>
        </div>
        {/* Unavailable stays focusable (aria-disabled), so a keyboard user reaches the reason as well as the pointer. */}
        <button
          ref={button}
          type="button"
          className="developer-open-log"
          data-force={force?.openLog}
          aria-disabled={unavailable !== undefined || undefined}
          aria-describedby={unavailable !== undefined ? why : undefined}
          title={unavailable}
          onClick={openLog}
        >
          {t("developer.openLogButton")}
        </button>
        {unavailable !== undefined && (
          <span id={why} className="heard-only">
            {unavailable}
          </span>
        )}
      </div>
      {/* Keyed by the active session, so a new one starts at its end, following, with no row selected; until the
          reader has the new session's lines it shows none of the one left (FLOW-READ-RAW-LOG-R04). */}
      <RawLogList key={session ?? ""} view={view} lines={shown} filtering={filtering} active={session !== undefined} backend={backend} unopened={unopened} force={force?.list} />
    </div>
  );
}

/** A length token in pixels, as the page resolves it; 0 when it is unset, as where no stylesheet is loaded. */
function pixels(el: Element, token: string): number {
  return Number.parseFloat(getComputedStyle(el).getPropertyValue(token)) || 0;
}

/**
 * Whether the toolbar is too narrow for one row (docs/design/WORKBENCH.md, The toolbar): one row while it can give the
 * field and the count twice `size-row-title-min` beside Open diagnostic log, else the button goes to a second row. It
 * follows the toolbar's width alone, never what is typed, so the list doesn't move while the user types.
 */
function useNarrowToolbar(toolbar: RefObject<HTMLDivElement | null>, button: RefObject<HTMLButtonElement | null>, forced?: boolean): boolean {
  const [narrow, setNarrow] = useState(forced ?? false);
  useLayoutEffect(() => {
    const el = toolbar.current;
    if (!el || forced !== undefined) return;
    const measure = () => {
      const style = getComputedStyle(el);
      const room = el.clientWidth - (Number.parseFloat(style.paddingLeft) || 0) - (Number.parseFloat(style.paddingRight) || 0);
      const needed = 2 * pixels(el, "--size-row-title-min") + pixels(el, "--space-sm") + (button.current?.offsetWidth ?? 0);
      setNarrow(room < needed);
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [toolbar, button, forced]);
  return narrow;
}
