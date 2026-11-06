import { useState } from "react";
import type { Project } from "../protocol/core-api.gen";
import { type BackendChoices, type SessionValues, available, seedScreen, startingProject, valuesFor } from "../session/controls";
import type { Sending } from "../session/follow";
import { AddProject } from "./AddProject";
import { Composer } from "./Composer";
import { type ControlKey, type Host, SessionControls } from "./SessionControls";
import "./NewSessionScreen.css";

interface NewSessionScreenProps {
  hosts: readonly Host[];
  /** The host Ersa runs on: its chip reads Local, and the screen opened for no project starts at its project. */
  localHostId: string;
  /** Most recently used first (FLOW-NEW-SESSION-R14). */
  projects: readonly Project[];
  /** Each host's backends from `backend.list`, by host id; a host absent here is still being detected. */
  backends: Readonly<Record<string, readonly BackendChoices[]>>;
  /** The values the user last started a session with, by project id. */
  lastUsed?: Readonly<Record<string, SessionValues>>;
  /** The project the user chose New session for; else the local host's most recently used (FLOW-NEW-SESSION-R15). */
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
 * one, asking whether the user trusts it first (until FLOW-NEW-SESSION-R16 to R20 are built).
 */
export function NewSessionScreen(props: NewSessionScreenProps) {
  const { hosts, localHostId, projects, backends, lastUsed = {}, onStart, starting, onAddProject, adding } = props;
  const first = startingProject(projects, localHostId, props.projectId);
  // Where the screen starts and what the user has chosen since; it starts afresh as seedScreen says.
  const [seed, setSeed] = useState(() => seedScreen(undefined, first, props.projectId, lastUsed));
  const now = seedScreen(seed, first, props.projectId, lastUsed);
  if (now !== seed) setSeed(now);
  const wanted = now.wanted;
  // A choice, a word written or a message sent holds the screen where the user saw it.
  const setWanted = (next: Partial<SessionValues>) => setSeed((s) => ({ ...s, touched: true, wanted: next }));
  const hold = () => setSeed((s) => (s.touched ? s : { ...s, touched: true }));
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
      // A host's own projects follow it; its most recently used one, at what was last used there.
      const next = startingProject(projects, id);
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
          onWrite={hold}
          onSend={(text) => {
            hold();
            return onStart(text, values);
          }}
        />
        <SessionControls
          hosts={hosts}
          localHostId={localHostId}
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
