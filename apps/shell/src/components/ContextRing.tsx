import { t } from "../catalog";
import { ringBand } from "../session/tile";
import "./ContextRing.css";

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
}

// The ring is drawn in a box as many units wide as the size token is pixels (`size-ring` 16, `size-ring-sm` 12),
// so one unit is one pixel and the hairline stroke stays a hairline at either size. `pathLength` makes the arc's
// length a percentage, whatever the radius.
const BOX = { ring: 16, "ring-sm": 12 } as const;

// The shortest arc the context ring draws, in percent of the ring: a twentieth, so a small share still shows
// (docs/design/COMPOSER.md, The context ring).
const MIN_ARC = 5;

/** The context ring (GLOSSARY): a gauge of context consumed, with its readout beside it once there is one. */
export function ContextRing({ used, size = "ring", beforeSession, turning, mark }: ContextRingProps) {
  // A value Core could not measure reads as none, not as NaN; and it never shows more than full.
  const share = used === undefined || !Number.isFinite(used) ? undefined : Math.min(Math.max(used, 0), 1);
  const percent = share === undefined ? undefined : Math.round(share * 100);
  // A share above nothing but under 1 percent reads as less than 1 percent, never as none or rounded up to 1 percent.
  const under1 = share !== undefined && share > 0 && share < 0.01;
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
  const arc = percent === undefined || share === 0 ? undefined : mark ? percent : Math.max(percent, MIN_ARC);
  const band = mark ? undefined : ringBand(share);
  const label =
    percent === undefined
      ? t(beforeSession ? "sessionControls.contextRing.noneLabel" : "sessionControls.contextRing.unreportedLabel")
      : under1
        ? t("sessionControls.contextRing.underOneLabel")
        : t("sessionControls.contextRing.usedLabel", { percent });
  return (
    <span
      className="context-ring"
      data-size={size}
      data-band={band === "quiet" ? undefined : band}
      role={mark ? undefined : "img"}
      aria-hidden={mark || undefined}
      aria-label={mark ? undefined : label}
      // Under the pointer the ring says what it says to assistive technology.
      title={mark ? undefined : label}
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
}
