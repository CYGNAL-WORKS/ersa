import { Funnel } from "lucide-react";
import { useId, useMemo, useRef, useState } from "react";
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
  force,
}: {
  /** The active session, or undefined while the new-session screen shows. */
  session: string | undefined;
  /** What the reader holds of the active session's raw log. */
  view: RawLogView | undefined;
  backend: string;
  /** For the gallery: the filter's text, the field with keyboard focus, and the list's states. */
  force?: { filter?: string; field?: "focus"; list?: Parameters<typeof RawLogList>[0]["force"] };
}) {
  const [filter, setFilter] = useState(force?.filter ?? "");
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
    <>
      <div className="developer-toolbar">
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
      {/* Keyed by the active session, so a new one starts at its end, following, with no row selected; until the
          reader has the new session's lines it shows none of the one left (FLOW-READ-RAW-LOG-R04). */}
      <RawLogList key={session ?? ""} view={view} lines={shown} filtering={filtering} active={session !== undefined} backend={backend} force={force?.list} />
    </>
  );
}
