import { useEffect, useRef, useState } from "react";
import type { Project } from "../protocol/core-api.gen";
import { type BackendChoices, type SessionValues, available, howFor, seedScreen, startingProject, valuesFor } from "../session/controls";
import type { Sending } from "../session/follow";
import { Composer } from "./Composer";
import { type ControlKey, type Host, SessionControls } from "./SessionControls";
import { TrustQuestion, folderName } from "./TrustQuestion";
import "./NewSessionScreen.css";

/**
 * What came of the folder dialog: the user canceled it, picked a folder that is already a project on the host
 * (`project.find`, FLOW-NEW-SESSION-R18), picked one that is not yet, or it failed, with the reason in `adding`.
 */
export type FolderChoice = { kind: "canceled" } | { kind: "found"; project: Project } | { kind: "new"; path: string } | { kind: "failed" };

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
  /**
   * Opens the system's folder dialog on the local host and finds whether the folder picked is already a project
   * there; absent while there is no dialog to open, and Add project is then unavailable.
   */
  onChooseFolder?: () => Promise<FolderChoice>;
  /** Adds the folder as a project on the local host, trusted or not; resolves to the project once Core added it. */
  onAddProject: (path: string, trusted: boolean) => Promise<Project | undefined>;
  /** Choosing or adding a folder: on its way, or why it failed. */
  adding?: Sending;
  /** For the gallery: focus not taken, the composer shown focused, a chip shown open or in a state. */
  autoFocus?: boolean;
  forceFocus?: boolean;
  open?: ControlKey;
  force?: { key: ControlKey; state: "hover" | "focus" | "pressed" };
  initialText?: string;
  /** For the gallery: the trust question already asked about this folder, with Cancel shown focused or not. */
  initialAsking?: string;
  forceCancelFocus?: boolean;
}

/**
 * The new-session screen (LAY-SHELL): the Ersa wordmark above the composer, focused, and the session controls
 * beneath it saying where and how the session will run (FLOW-NEW-SESSION-R01). The project chip's Add project…, or
 * Ctrl+O, adds a project from the folder dialog, asking first whether the user trusts a folder that is not yet one
 * (FLOW-NEW-SESSION-R16 to R20); with no project, the chip and sending both open the dialog.
 */
export function NewSessionScreen(props: NewSessionScreenProps) {
  const { hosts, localHostId, backends, lastUsed = {}, onStart, starting, onChooseFolder, onAddProject, adding } = props;
  // A project just added or found is chosen at once, before Core's list is read again with it (FLOW-NEW-SESSION-R16).
  const [added, setAdded] = useState<Project>();
  const projects = added && !props.projects.some((p) => p.id === added.id) ? [added, ...props.projects] : props.projects;
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
  const hostId = project?.host_id ?? localHostId;
  const busy = starting?.state === "sending";

  // The folder the trust question asks about, while it is open.
  const [asking, setAsking] = useState(props.initialAsking);
  // While the dialog is open, or a folder is being found or added, another Add project does nothing.
  const choosing = useRef(false);
  const words = useRef<HTMLTextAreaElement>(null);
  const backToWords = () => words.current?.focus({ preventScroll: true });
  const chooseProject = (id: string) => setWanted({ ...lastUsed[id], project_id: id });

  // The folder dialog is the local host's (FLOW-NEW-SESSION-R16, R19): a remote host's folders are not this computer's.
  const canAdd = onChooseFolder !== undefined && hostId === localHostId && !busy && asking === undefined;
  const addProject = async () => {
    if (!canAdd || choosing.current) return;
    choosing.current = true;
    try {
      const choice = await onChooseFolder();
      // A folder already a project is chosen without the question (R18); a new one is asked about first (R16).
      if (choice.kind === "found") {
        // Listed at once, as an added one is, in case the list Core last gave does not have it yet.
        setAdded(choice.project);
        chooseProject(choice.project.id);
      }
      if (choice.kind === "new") setAsking(choice.path);
      else backToWords();
    } finally {
      choosing.current = false;
    }
  };
  // Answered, the project is added with the answer and chosen; canceled, nothing changes (FLOW-NEW-SESSION-R17).
  const answer = async (trusted: boolean | undefined) => {
    const path = asking;
    setAsking(undefined);
    backToWords();
    if (path === undefined || trusted === undefined || choosing.current) return;
    choosing.current = true;
    try {
      const project = await onAddProject(path, trusted);
      if (project) {
        setAdded(project);
        chooseProject(project.id);
      }
    } finally {
      choosing.current = false;
    }
  };

  // Ctrl+O chooses Add project from anywhere on the screen (FLOW-NEW-SESSION-R16). The listener is added once and
  // calls the latest `addProject` through a ref.
  const shortcut = useRef(addProject);
  shortcut.current = addProject;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey && event.key.toLowerCase() === "o") {
        // The keys are Add project's on this screen, never the webview's own.
        event.preventDefault();
        void shortcut.current();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  const hostBackends = backends[hostId];
  // With no project yet, the where values are the local host's alone; the how values still show (R19).
  const values: SessionValues = project
    ? valuesFor(project, hostBackends ?? [], wanted)
    : { host_id: localHostId, project_id: "", workspace: "root", ...howFor(hostBackends ?? [], wanted) };

  const onChoose = (key: ControlKey, id: string) => {
    if (key === "host") {
      // A host's own projects follow it; its most recently used one, at what was last used there.
      const next = startingProject(projects, id);
      if (next) chooseProject(next.id);
    } else if (key === "project") {
      chooseProject(id);
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
        {/* In the status line's place above the box (docs/design/COMPOSER.md, The trust question). */}
        {asking !== undefined && (
          <TrustQuestion
            name={folderName(asking)}
            path={asking}
            purpose="add"
            onAnswer={(trusted) => void answer(trusted)}
            onDismiss={() => void answer(undefined)}
            takeFocus={props.initialAsking === undefined}
            forceCancelFocus={props.forceCancelFocus}
          />
        )}
        {asking === undefined && adding?.state === "failed" && (
          <p className="problem new-session-problem" role="alert">
            {adding.message}
          </p>
        )}
        <Composer
          inputRef={words}
          autoFocus={props.autoFocus ?? true}
          force={props.forceFocus ? "focus" : undefined}
          initialText={props.initialText}
          unavailable={unavailable}
          sending={starting}
          onWrite={hold}
          onSend={(text) => {
            hold();
            // With no project, sending opens the folder dialog and the message stays for the user to send (R19).
            if (!project) {
              void addProject();
              return Promise.resolve(false);
            }
            return onStart(text, values);
          }}
        />
        <SessionControls
          hosts={hosts}
          localHostId={localHostId}
          projects={projects}
          backends={hostBackends}
          values={values}
          onChoose={onChoose}
          onAddProject={canAdd ? () => void addProject() : undefined}
          disabled={busy || adding?.state === "sending"}
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
