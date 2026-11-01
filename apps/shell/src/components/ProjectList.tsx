import { type ReactNode, useState } from "react";
import type { ProjectsView } from "../core/connection";
import type { Project } from "../protocol/core-api.gen";
import type { Sending } from "../session/follow";
import { Button } from "./Button";
import { TrustQuestion } from "./TrustQuestion";
import "./ProjectList.css";

interface ProjectListProps {
  view: ProjectsView;
  /** Starts a session in a new workspace of the project. Offered for git projects, whose workspaces are worktrees. */
  onNewSession?: (project: Project) => void;
  /** Sessions being started, by project id, and why one could not be. */
  starting?: Record<string, Sending>;
  /** Changes whether the user trusts the project. Trusting asks first, saying what trust lets run; withdrawing trust does not. */
  onSetTrust?: (project: Project, trusted: boolean) => void;
  /** Trust being changed, by project id, and why it could not be. */
  trusting?: Record<string, Sending>;
  /** For the gallery: the project whose trust question is showing. */
  initialAsking?: string;
  /** Shown beneath the list: the way to add a project. */
  children?: ReactNode;
}

/** The projects Core knows, with their loading, empty and error states. */
export function ProjectList({ view, onNewSession, starting = {}, onSetTrust, trusting = {}, initialAsking, children }: ProjectListProps) {
  // The project whose trust question is showing, if any.
  const [asking, setAsking] = useState(initialAsking);
  return (
    <section className="projects" aria-label="Projects">
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
            const start = starting[p.id];
            const trust = trusting[p.id];
            const changingTrust = trust?.state === "sending";
            const newSession = onNewSession && p.vcs === "git";
            return (
              <li key={p.id} className="project">
                <span className="project-name">{p.name}</span>
                <span className="project-vcs">{p.vcs === "none" ? "" : p.vcs}</span>
                <span className="project-path mono" title={p.path}>
                  {p.path}
                </span>
                <span className="project-trust">{p.trusted ? "Trusted" : "Not trusted"}</span>
                {(newSession || onSetTrust) && (
                  <div className="project-action">
                    {newSession && (
                      <Button onClick={() => onNewSession(p)} disabled={start?.state === "sending"}>
                        {start?.state === "sending" ? "Starting…" : "New session"}
                      </Button>
                    )}
                    {onSetTrust &&
                      (p.trusted ? (
                        <Button onClick={() => onSetTrust(p, false)} disabled={changingTrust}>
                          Don’t trust
                        </Button>
                      ) : (
                        <Button onClick={() => setAsking(p.id)} disabled={changingTrust || asking === p.id}>
                          Trust…
                        </Button>
                      ))}
                  </div>
                )}
                {asking === p.id && onSetTrust && (
                  <div className="project-question">
                    <TrustQuestion
                      name={p.name}
                      busy={changingTrust}
                      onAnswer={(trusted) => {
                        setAsking(undefined);
                        if (trusted) onSetTrust(p, true);
                      }}
                    />
                  </div>
                )}
                {start?.state === "failed" && (
                  <p className="problem project-problem" role="alert">
                    {start.message}
                  </p>
                )}
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
