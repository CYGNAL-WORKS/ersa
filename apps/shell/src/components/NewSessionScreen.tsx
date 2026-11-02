import { useState } from "react";
import type { Project } from "../protocol/core-api.gen";
import { type BackendChoices, type SessionValues, available, valuesFor } from "../session/controls";
import type { Sending } from "../session/follow";
import { AddProject } from "./AddProject";
import { Composer } from "./Composer";
import { type ControlKey, type Host, SessionControls } from "./SessionControls";
import "./NewSessionScreen.css";

interface NewSessionScreenProps {
  hosts: Host[];
  projects: Project[];
  /** Each host's backends from `backend.list`, by host id; a host absent here is still being detected. */
  backends: Record<string, BackendChoices[]>;
  /** The values the user last started a session with, by project id. */
  lastUsed?: Record<string, SessionValues>;
  /** The project the user chose New session for; else the first. */
  projectId?: string;
  /** Starts the session with the first message; resolves true once it has started. */
  onStart: (text: string, values: SessionValues) => Promise<boolean>;
  starting?: Sending;
  /** Adds a project, once the user has said whether they trust it; offered while there is none. */
  onAddProject: (path: string, trusted: boolean) => Promise<boolean>;
  adding?: Sending;
  /** For the gallery: focus not taken, the composer shown focused, a chip shown open or in a state. */
  autoFocus?: boolean;
  forceFocus?: boolean;
  open?: ControlKey;
  force?: { key: ControlKey; state: "hover" | "focus" | "pressed" };
  initialText?: string;
}

/**
 * The new-session screen (LAY-SHELL): the Ersa wordmark above the composer, focused, and the session controls
 * beneath it saying where and how the session will run (FLOW-NEW-SESSION-R01). With no project it offers to add
 * one, asking whether the user trusts it first (FLOW-NEW-SESSION-R10).
 */
export function NewSessionScreen(props: NewSessionScreenProps) {
  const { hosts, projects, backends, lastUsed = {}, onStart, starting, onAddProject, adding } = props;
  const first = projects.find((p) => p.id === props.projectId) ?? projects[0];
  // What the user has chosen on this screen; the controls show it where the project and its host offer it.
  const [wanted, setWanted] = useState<Partial<SessionValues>>(() => (first ? (lastUsed[first.id] ?? {}) : {}));
  const project = projects.find((p) => p.id === wanted.project_id) ?? first;

  if (!project) {
    return (
      <section className="new-session" aria-label="New session">
        <Wordmark />
        <div className="new-session-add">
          <p className="new-session-lead">Add a project to start a session: a folder on this computer.</p>
          <AddProject onAdd={onAddProject} adding={adding} />
        </div>
      </section>
    );
  }

  const hostBackends = backends[project.host_id];
  const values = valuesFor(project, hostBackends ?? [], wanted);
  const busy = starting?.state === "sending";

  const choose = (key: ControlKey, id: string) => {
    if (key === "host") {
      // A host's own projects follow it; its first one, at what was last used there.
      const next = projects.find((p) => p.host_id === id);
      if (next) setWanted({ ...lastUsed[next.id], project_id: next.id });
    } else if (key === "project") {
      setWanted({ ...lastUsed[id], project_id: id });
    } else if (key === "backend") {
      // A model or effort chosen for one backend means nothing to another.
      setWanted({ ...values, backend: id, model: undefined, effort: undefined, permission_mode: undefined });
    } else {
      setWanted({ ...values, [key]: id });
    }
  };

  const unavailable =
    hostBackends === undefined
      ? "Detecting the backends on this host…"
      : available(hostBackends).length === 0
        ? "No backend is available on this host."
        : undefined;

  return (
    <section className="new-session" aria-label="New session">
      <Wordmark />
      <div className="new-session-start">
        <Composer
          autoFocus={props.autoFocus ?? true}
          force={props.forceFocus ? "focus" : undefined}
          initialText={props.initialText}
          unavailable={unavailable}
          sending={starting}
          onSend={(text) => onStart(text, values)}
        />
        <SessionControls
          hosts={hosts}
          projects={projects}
          backends={hostBackends}
          values={values}
          onChoose={choose}
          disabled={busy}
          open={props.open}
          force={props.force}
        />
      </div>
    </section>
  );
}

function Wordmark() {
  return <h1 className="new-session-wordmark">Ersa</h1>;
}
