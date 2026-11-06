import { t } from "../catalog";
import { nearFull } from "../session/tile";
import "./ContextRing.css";

interface ContextRingProps {
  /** The share of the context window consumed, from 0 to 1; absent before the session has used any. */
  used?: number;
  /** `ring-sm` is a tile's ring, drawn without its readout; `ring` is the session controls', with it. */
  size?: "ring" | "ring-sm";
}

// The ring is drawn in a box as many units wide as the size token is pixels (`size-ring` 16, `size-ring-sm` 12),
// so one unit is one pixel and the hairline stroke stays a hairline at either size. `pathLength` makes the arc's
// length a percentage, whatever the radius.
const BOX = { ring: 16, "ring-sm": 12 } as const;

/** The context ring (GLOSSARY): a gauge of context consumed, with its readout beside it once there is one. */
export function ContextRing({ used, size = "ring" }: ContextRingProps) {
  // A value Core could not measure reads as none, not as NaN.
  const share = used === undefined || !Number.isFinite(used) ? undefined : Math.min(Math.max(used, 0), 1);
  const percent = share === undefined ? undefined : Math.round(share * 100);
  const box = BOX[size];
  const centre = box / 2;
  const radius = centre - 1;
  return (
    <span
      className="context-ring"
      data-size={size}
      data-near-full={nearFull(share) || undefined}
      role="img"
      aria-label={percent === undefined ? t("sessionControls.contextRing.noneLabel") : t("sessionControls.contextRing.usedLabel", { percent })}
    >
      <svg className="context-ring-gauge" viewBox={`0 0 ${box} ${box}`} aria-hidden="true">
        <circle className="context-ring-track" cx={centre} cy={centre} r={radius} />
        {percent !== undefined && percent > 0 && (
          <circle
            className="context-ring-used"
            cx={centre}
            cy={centre}
            r={radius}
            pathLength={100}
            strokeDasharray={`${percent} 100`}
            // From twelve o'clock, clockwise, as a gauge reads.
            transform={`rotate(-90 ${centre} ${centre})`}
          />
        )}
      </svg>
      {percent !== undefined && size === "ring" && <span className="readout context-ring-readout">{`${percent}%`}</span>}
    </span>
  );
}
