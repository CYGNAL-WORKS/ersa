import "./ContextRing.css";

interface ContextRingProps {
  /** The share of the context window consumed, from 0 to 1; absent before the session has used any. */
  used?: number;
}

// The ring is drawn in a 16-unit box at the size of the `space-lg` token, so one unit is one pixel and the
// hairline stroke stays a hairline. `pathLength` makes the arc's length a percentage, whatever the radius.
const CENTRE = 8;
const RADIUS = 7;

/** The context ring (GLOSSARY): a gauge of context consumed, with its readout beside it once there is one. */
export function ContextRing({ used }: ContextRingProps) {
  // A value Core could not measure reads as none, not as NaN.
  const percent = used === undefined || !Number.isFinite(used) ? undefined : Math.round(Math.min(Math.max(used, 0), 1) * 100);
  return (
    <span
      className="context-ring"
      role="img"
      aria-label={percent === undefined ? "Context: none used yet" : `Context: ${percent} percent used`}
    >
      <svg className="context-ring-gauge" viewBox="0 0 16 16" aria-hidden="true">
        <circle className="context-ring-track" cx={CENTRE} cy={CENTRE} r={RADIUS} />
        {percent !== undefined && percent > 0 && (
          <circle
            className="context-ring-used"
            cx={CENTRE}
            cy={CENTRE}
            r={RADIUS}
            pathLength={100}
            strokeDasharray={`${percent} 100`}
            // From twelve o'clock, clockwise, as a gauge reads.
            transform={`rotate(-90 ${CENTRE} ${CENTRE})`}
          />
        )}
      </svg>
      {percent !== undefined && <span className="readout context-ring-readout">{`${percent}%`}</span>}
    </span>
  );
}
