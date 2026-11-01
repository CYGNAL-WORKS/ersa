import type { ReactNode } from "react";
import type { ProjectsView } from "../core/connection";
import type { Project } from "../protocol/core-api.gen";
import type { Sending } from "../session/follow";
import { Button } from "./Button";
import "./ProjectList.css";

interface ProjectListProps {
  view: ProjectsView;
  /** Starts a session in a new workspace of the project. Offered for git projects, whose workspaces are worktrees. */
  onNewSession?: (project: Project) => void;
  /** Sessions being started, by project id, and why one could not be. */
  starting?: Record<string, Sending>;
  /** Shown beneath the list: the way to add a project. */
  children?: ReactNode;
}

/** The projects Core knows, with their loading, empty and error states. */
export function ProjectList({ view, onNewSession, starting = {}, children }: ProjectListProps) {
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
            return (
              <li key={p.id} className="project">
                <span className="project-name">{p.name}</span>
                <span className="project-vcs">{p.vcs === "none" ? "" : p.vcs}</span>
                <span className="project-path mono" title={p.path}>
                  {p.path}
                </span>
                {onNewSession && p.vcs === "git" && (
                  <div className="project-action">
                    <Button onClick={() => onNewSession(p)} disabled={start?.state === "sending"}>
                      {start?.state === "sending" ? "Starting…" : "New session"}
                    </Button>
                  </div>
                )}
                {start?.state === "failed" && (
                  <p className="problem project-problem" role="alert">
                    {start.message}
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
