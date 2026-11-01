import { useCallback, useEffect, useState } from "react";
import "./App.css";
import { CoreStatus } from "./components/CoreStatus";
import { ProjectList } from "./components/ProjectList";
import type { CoreClient } from "./core/client";
import { type CoreView, type ProjectsView, connect } from "./core/connection";

/** The projects part of the view, whatever phase Core is in. */
function projectsOf(view: CoreView): ProjectsView {
  switch (view.phase) {
    case "connected":
      return view.projects;
    case "lost":
    case "failed":
      return { state: "unavailable" };
    case "connecting":
      return { state: "loading" };
  }
}

export function App({ client }: { client: CoreClient }) {
  const [view, setView] = useState<CoreView>({ phase: "connecting" });
  // Bumped to attach again; the effect below reconnects on each change.
  const [attempt, setAttempt] = useState(0);
  const retry = useCallback(() => setAttempt((n) => n + 1), []);

  useEffect(() => connect(client, setView), [client, attempt]);

  return (
    <div className="app">
      <nav className="rail" aria-label="Rail">
        <div className="rail-projects">
          <ProjectList view={projectsOf(view)} />
        </div>
        <CoreStatus view={view} onRetry={retry} />
      </nav>
      <main className="stage">No session open.</main>
    </div>
  );
}
