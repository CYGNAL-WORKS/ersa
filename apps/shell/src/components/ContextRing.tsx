import { type CSSProperties, useId, useState } from "react";
import { t } from "../catalog";
import { type Reading, type UsageWindow, contextReading, nearest, readings, resetsIn } from "../session/limits";
import { ringBand } from "../session/tile";
import "./ContextRing.css";
import "./Tip.css";

interface ContextRingProps {
  /** The share of the context window consumed, from 0 to 1; absent while none is reported. */
  used?: number;
  /** `ring-sm` is a tile's ring, drawn without its readout; `ring` is the session controls', with it. */
  size?: "ring" | "ring-sm";
  /**
   * Before a session exists, as on the new-session screen: with no reading the ring says none is used yet, since
   * there is nothing to report, rather than that context isn't reported yet.
   */
  beforeSession?: boolean;
  /**
   * Waiting on work of unknown length, as while backends are detected: the track with a quarter arc in ink-muted,
   * turning once per motion-loop-ring (docs/design/COMPOSER.md, The session controls). A mark beside words that say
   * what is waited on, so it carries no name of its own.
   */
  turning?: boolean;
  /** A gauge beside words that give its share, as a send's copy (FLOW-SEND-MESSAGE-R16): no readout, and no name of its own. */
  mark?: boolean;
  /**
   * The session controls' ring, given the usage windows the session's backend reported (none on the new-session
   * screen): it reads the nearest limit, the context or a window, names it, and under the pointer or with keyboard
   * focus lists every reading (LAY-SHELL, Context). Absent, as on a tile and the shelf, it reads the context alone.
   */
  windows?: readonly UsageWindow[];
  /** For the gallery: the ring with keyboard focus, its list shown. */
  force?: "focus";
  /** For the gallery: the time the list's reset times count from, in place of the time the list opened. */
  now?: Date;
}

// The ring is drawn in a box as many units wide as the size token is pixels (`size-ring` 16, `size-ring-sm` 12),
// so one unit is one pixel and the hairline stroke stays a hairline at either size. `pathLength` makes the arc's
// length a percentage, whatever the radius.
const BOX = { ring: 16, "ring-sm": 12 } as const;

// The shortest arc the context ring draws, in percent of the ring: a twentieth, so a small share still shows
// (docs/design/COMPOSER.md, The context ring).
const MIN_ARC = 5;

/** What the ring says of a reading, to assistive technology and as a line of its list. */
function readingLabel(reading: Reading, beforeSession: boolean | undefined): string {
  const { label, percent, under1 } = reading;
  if (label !== undefined) return t("sessionControls.contextRing.windowUsedLabel", { label, percent: percent ?? 0 });
  if (percent === undefined) return t(beforeSession ? "sessionControls.contextRing.noneLabel" : "sessionControls.contextRing.unreportedLabel");
  return under1 ? t("sessionControls.contextRing.underOneLabel") : t("sessionControls.contextRing.usedLabel", { percent });
}

/**
 * The context ring (GLOSSARY): a gauge of context consumed, with its readout beside it once there is one. In the
 * session controls it reads the nearest limit instead, the context or a usage window, and lists every reading.
 */
export function ContextRing({ used, size = "ring", beforeSession, turning, mark, windows, force, now: fixedNow }: ContextRingProps) {
  const id = useId();
  // When the list's reset times are counted from: when the pointer or focus last arrived, so the time left is read
  // as the list opens rather than when the last report came.
  const [opened, setOpened] = useState(() => new Date());
  const now = fixedNow ?? opened;
  const list = windows ? readings(used, windows) : [contextReading(used)];
  const shown = nearest(list);
  const { percent } = shown;
  // A share above nothing but under 1 percent reads as less than 1 percent, never as none or rounded up to 1 percent.
  const under1 = shown.under1 === true;
  const box = BOX[size];
  const centre = box / 2;
  const radius = centre - 1;
  if (turning) {
    return (
      <span className="context-ring" data-size={size} data-turning="" aria-hidden="true">
        <svg className="context-ring-gauge" viewBox={`0 0 ${box} ${box}`}>
          <circle className="context-ring-track" cx={centre} cy={centre} r={radius} />
          <circle className="context-ring-used" cx={centre} cy={centre} r={radius} pathLength={4} strokeDasharray="1 3" />
        </svg>
      </span>
    );
  }
  // The send's copy gauge fills to the share itself (COMPOSER.md, A message is on its way): no band, no shortest arc.
  const arc = percent === undefined || (percent === 0 && !under1) ? undefined : mark ? percent : Math.max(percent, MIN_ARC);
  // The band follows the whole percent shown, so the tone and the figure never disagree (LAY-SHELL, Context).
  const band = mark || percent === undefined ? undefined : ringBand(percent / 100);
  const label = readingLabel(shown, beforeSession);
  const anchor = `--context-ring-${id.replace(/[^a-zA-Z0-9]/g, "")}`;
  const tipId = `${id}-tip`;
  const ring = (
    <span
      className="context-ring"
      data-size={size}
      data-band={band === "quiet" ? undefined : band}
      data-limits={windows ? "" : undefined}
      data-force={force}
      role={mark ? undefined : "img"}
      aria-hidden={mark || undefined}
      aria-label={mark ? undefined : label}
      // On a tile and the shelf, under the pointer the ring says what it says to assistive technology; in the session
      // controls its list says that and more, and keyboard focus reaches it.
      title={mark || windows ? undefined : label}
      tabIndex={windows ? 0 : undefined}
      aria-describedby={windows ? tipId : undefined}
      style={windows ? ({ anchorName: anchor } as CSSProperties) : undefined}
      onPointerEnter={windows ? () => setOpened(new Date()) : undefined}
      onFocus={windows ? () => setOpened(new Date()) : undefined}
    >
      <svg className="context-ring-gauge" viewBox={`0 0 ${box} ${box}`} aria-hidden="true">
        <circle className="context-ring-track" cx={centre} cy={centre} r={radius} />
        {/* Always drawn, at no length while there is nothing to show, so a first reading grows the arc rather than popping it in. */}
        <circle
          className="context-ring-used"
          cx={centre}
          cy={centre}
          r={radius}
          pathLength={100}
          strokeDasharray={`${arc ?? 0} 100`}
          // From twelve o'clock, clockwise, as a gauge reads.
          transform={`rotate(-90 ${centre} ${centre})`}
        />
      </svg>
      {percent !== undefined && size === "ring" && !mark && (
        <span className="readout context-ring-readout">{under1 ? t("sessionControls.contextRing.underOneReadout", { percent: 1 }) : `${percent}%`}</span>
      )}
    </span>
  );
  if (!windows) return ring;
  return (
    <>
      {ring}
      {/* Every reading, one to a line and never re-sorted: the context, then each window as reported. */}
      <span id={tipId} className="tip context-ring-tip" role="tooltip" style={{ positionAnchor: anchor } as CSSProperties}>
        {list.map((reading, i) => {
          const resets = resetsIn(reading.resetsAt, now);
          return (
            // The context is first and each window keeps its place, so the index is a stable key.
            // biome-ignore lint/suspicious/noArrayIndexKey: the list is never re-sorted
            <span key={i} className="context-ring-tip-line">
              <span>{readingLabel(reading, beforeSession)}</span>
              {resets && <span className="context-ring-tip-resets">{resets}</span>}
            </span>
          );
        })}
      </span>
    </>
  );
}
