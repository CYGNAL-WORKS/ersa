import { type ReactNode, useEffect, useRef, useState } from "react";
import type { ProjectsView } from "../core/connection";
import type { Project } from "../protocol/core-api.gen";
import type { Sending } from "../session/follow";
import { Button } from "./Button";
import { TrustQuestion } from "./TrustQuestion";
import "./ProjectList.css";

interface ProjectListProps {
  view: ProjectsView;
  /** Opens the new-session screen for the project; nothing is created until its first message (FLOW-NEW-SESSION-R03). */
  onNewSession?: (project: Project) => void;
  /** Changes whether the user trusts the project. Trusting asks first, saying what trust lets run; withdrawing trust does not. */
  onSetTrust?: (project: Project, trusted: boolean) => void;
  /** Trust being changed, by project id, and why it could not be. */
  trusting?: Record<string, Sending>;
  /** A line to show under a project, by project id: that its running sessions keep the trust they started with. */
  notes?: Record<string, string>;
  /** For the gallery: the project whose trust question is showing. */
  initialAsking?: string;
  /** Shown beneath the list: the way to add a project. */
  children?: ReactNode;
}

/** The projects Core knows, with their loading, empty and error states. */
export function ProjectList({
  view,
  onNewSession,
  onSetTrust,
  trusting = {},
  notes = {},
  initialAsking,
  children,
}: ProjectListProps) {
  // The project whose trust question is showing, if any.
  const [asking, setAsking] = useState(initialAsking);
  // Once a question closes, focus returns to the button that opened it. After "Trust" that button is
  // disabled while the answer is saved, so focus waits for it, unless the user has moved focus meanwhile.
  const list = useRef<HTMLElement>(null);
  const closedFor = useRef<string>(undefined);
  const close = (id: string) => {
    closedFor.current = id;
    setAsking(undefined);
  };
  useEffect(() => {
    const id = closedFor.current;
    if (!id) return;
    const button = list.current?.querySelector<HTMLButtonElement>(`[data-trust-for="${CSS.escape(id)}"]`);
    if (button?.disabled) return;
    closedFor.current = undefined;
    const lost = document.activeElement === null || document.activeElement === document.body;
    if (lost) button?.focus();
  }, [asking, trusting]);
  return (
    <section ref={list} className="projects" aria-label="Projects">
      <h2 className="projects-header">
        <span>Projects</span>
        {view.state === "ready" && <span className="readout">{view.projects.length}</span>}
      </h2>
      {view.state === "loading" && <p className="projects-note">Loading…</p>}
      {view.state === "unavailable" && <p className="projects-note">Projects appear once Core is connected.</p>}
      {view.state === "error" && (
        <p className="problem" role="alert">
          {view.message}
        </p>
      )}
      {view.state === "ready" && view.projects.length === 0 && (
        <p className="projects-note">
          No projects yet.
          <span className="projects-note-hint">Projects added to Core appear here.</span>
        </p>
      )}
      {view.state === "ready" && view.projects.length > 0 && (
        <ul className="projects-list">
          {view.projects.map((p) => {
            const trust = trusting[p.id];
            const changingTrust = trust?.state === "sending";
            return (
              <li key={p.id} className="project">
                <span className="project-name">{p.name}</span>
                <span className="project-vcs">{p.vcs === "none" ? "" : p.vcs}</span>
                <span className="project-path mono" title={p.path}>
                  {p.path}
                </span>
                <span className="project-trust">{p.trusted ? "Trusted" : "Not trusted"}</span>
                {(onNewSession || onSetTrust) && (
                  <div className="project-action">
                    {onNewSession && <Button onClick={() => onNewSession(p)}>New session</Button>}
                    {onSetTrust &&
                      (p.trusted ? (
                        <Button data-trust-for={p.id} onClick={() => onSetTrust(p, false)} disabled={changingTrust}>
                          {changingTrust ? "Saving…" : "Don’t trust"}
                        </Button>
                      ) : (
                        <Button data-trust-for={p.id} onClick={() => setAsking(p.id)} disabled={changingTrust || asking === p.id}>
                          {changingTrust ? "Saving…" : "Trust…"}
                        </Button>
                      ))}
                  </div>
                )}
                {asking === p.id && onSetTrust && (
                  <div className="project-question">
                    <TrustQuestion
                      name={p.name}
                      purpose="change"
                      takeFocus={p.id !== initialAsking}
                      onAnswer={(trusted) => {
                        close(p.id);
                        if (trusted) onSetTrust(p, true);
                      }}
                      onDismiss={() => close(p.id)}
                    />
                  </div>
                )}
                {/* Always there, so a screen reader announces the text when it appears. */}
                <p className="project-note" role="status">
                  {notes[p.id]}
                </p>
                {trust?.state === "failed" && (
                  <p className="problem project-problem" role="alert">
                    {trust.message}
                  </p>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {children}
    </section>
  );
}
