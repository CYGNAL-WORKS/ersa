import type { ReactNode } from "react";
import "./StatusTile.css";

export type Tone = "pending" | "positive" | "danger";

export interface Readout {
  label: string;
  value: string | number;
}

interface StatusTileProps {
  title: string;
  tone: Tone;
  summary: string;
  detail?: string;
  readouts?: Readout[];
  action?: ReactNode;
}

/** Glyph, title and summary on one line; detail, readouts and an action beneath. */
export function StatusTile({ title, tone, summary, detail, readouts, action }: StatusTileProps) {
  return (
    <section className="tile" data-tone={tone} aria-label={title}>
      <span className="glyph tile-glyph" data-tone={tone} aria-hidden="true" />
      <div>
        <span className="tile-title">{title}</span> <span className="tile-summary">{summary}</span>
      </div>
      {(detail || readouts?.length || action) && (
        <div className="tile-body">
          {detail && <p className="tile-detail">{detail}</p>}
          {readouts && readouts.length > 0 && (
            <dl className="tile-readouts">
              {readouts.map((r) => (
                <div key={r.label} className="tile-readout">
                  <dt>{r.label}</dt>
                  <dd className="readout">{r.value}</dd>
                </div>
              ))}
            </dl>
          )}
          {action && <div className="tile-action">{action}</div>}
        </div>
      )}
    </section>
  );
}
