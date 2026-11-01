// The development-only gallery (ADR 0008): every component in every state,
// light and dark side by side. The screenshot tests in e2e/ photograph each
// component section in each theme, so a visual change shows up in review.
// A component built for the window gets an entry here in the same change.

import type { ReactNode } from "react";
import { Button } from "../components/Button";
import { CoreStatus } from "../components/CoreStatus";
import { ProjectList } from "../components/ProjectList";
import type { CoreView } from "../core/connection";
import type { CoreHelloResult, Project } from "../protocol/core-api.gen";
import "./Gallery.css";

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "WORKSTATION", platform: "windows" },
  capabilities: [],
};

const projects: Project[] = [
  { id: "p1", host_id: "local", path: "D:\\Projects\\ersa", name: "ersa", vcs: "git" },
  { id: "p2", host_id: "local", path: "D:\\Projects\\game\\engine\\source\\runtime", name: "engine", vcs: "perforce" },
  { id: "p3", host_id: "local", path: "C:\\Users\\me\\notes", name: "notes with a rather long project name", vcs: "none" },
];

const coreViews: [string, CoreView][] = [
  ["Connecting", { phase: "connecting" }],
  [
    "Started",
    {
      phase: "connected",
      how: "launched",
      hello,
      status: { sessions_running: 3, sessions_waiting: 1, uptime_s: 42, db_path: "" },
      projects: { state: "loading" },
    },
  ],
  ["Attached, before status", { phase: "connected", how: "attached", hello, projects: { state: "loading" } }],
  ["Lost", { phase: "lost", reason: "Core closed the connection", hello }],
  [
    "Unavailable",
    {
      phase: "failed",
      message:
        "Core exited before it started serving (exit code: 1):\nersa-core: cannot open the database at C:\\Users\\me\\AppData\\Local\\Ersa\\ersa.db",
    },
  ],
];

interface Entry {
  name: string;
  states: [string, ReactNode][];
}

const entries: Entry[] = [
  {
    name: "CoreStatus",
    states: coreViews.map(([label, view]) => [label, <CoreStatus view={view} onRetry={() => {}} />]),
  },
  {
    name: "ProjectList",
    states: [
      ["Loading", <ProjectList view={{ state: "loading" }} />],
      ["Empty", <ProjectList view={{ state: "ready", projects: [] }} />],
      ["Projects", <ProjectList view={{ state: "ready", projects }} />],
      ["Error", <ProjectList view={{ state: "error", message: "Not connected to Core." }} />],
    ],
  },
  {
    name: "Button",
    states: [
      ["Default", <Button>Reconnect</Button>],
      ["Hover", <Button force="hover">Reconnect</Button>],
      ["Focus", <Button force="focus">Reconnect</Button>],
      ["Pressed", <Button force="pressed">Reconnect</Button>],
      ["Disabled", <Button disabled>Reconnect</Button>],
    ],
  },
];

export function Gallery() {
  return (
    <div className="gallery">
      {(["light", "dark"] as const).map((theme) => (
        <div key={theme} className="gallery-theme" data-theme={theme}>
          {entries.map((entry) => (
            <section key={entry.name} className="gallery-entry" data-component={entry.name}>
              <h2 className="gallery-name">
                {entry.name} <span className="gallery-theme-name">{theme}</span>
              </h2>
              <div className="gallery-states">
                {entry.states.map(([label, node]) => (
                  <figure key={label} className="gallery-state">
                    <figcaption className="gallery-label">{label}</figcaption>
                    <div className="gallery-specimen">{node}</div>
                  </figure>
                ))}
              </div>
            </section>
          ))}
        </div>
      ))}
    </div>
  );
}
