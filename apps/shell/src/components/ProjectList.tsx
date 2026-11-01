import type { ProjectsView } from "../core/connection";
import "./ProjectList.css";

/** The projects Core knows, with their loading, empty and error states. */
export function ProjectList({ view }: { view: ProjectsView }) {
  return (
    <section className="projects" aria-label="Projects">
      <h2 className="projects-header">
        <span>Projects</span>
        {view.state === "ready" && <span className="readout">{view.projects.length}</span>}
      </h2>
      {view.state === "loading" && <p className="projects-note">Loading…</p>}
      {view.state === "error" && (
        <p className="projects-error" role="alert">
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
          {view.projects.map((p) => (
            <li key={p.id} className="project">
              <span className="project-name">{p.name}</span>
              <span className="project-vcs">{p.vcs === "none" ? "" : p.vcs}</span>
              <span className="project-path mono" title={p.path}>
                {p.path}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
