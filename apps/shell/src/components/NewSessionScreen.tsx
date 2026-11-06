import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Project } from "../protocol/core-api.gen";
import {
  type BackendChoices,
  type Branches,
  type SessionValues,
  available,
  baseFor,
  howFor,
  seedScreen,
  startingProject,
  valuesFor,
} from "../session/controls";
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
  /** Adds the folder as a trusted project on the local host (FLOW-NEW-SESSION-R25); resolves to the project once Core added it. */
  onAddProject: (path: string) => Promise<Project | undefined>;
  /**
   * Reads a git project's local branches, for the base branch a new workspace starts from (FLOW-NEW-SESSION-R21);
   * resolves to why they could not be read rather than failing. Absent, the workspace chip offers no base branch.
   */
  onListBranches?: (projectId: string) => Promise<Branches>;
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
 * (FLOW-NEW-SESSION-R17 to R20, R27); with no project, the chip and sending both open the dialog.
 */
export function NewSessionScreen(props: NewSessionScreenProps) {
  const { hosts, localHostId, backends, lastUsed = {}, onStart, starting, onChooseFolder, onAddProject, adding } = props;
  // A project just added or found is chosen at once, before Core's list is read again with it (FLOW-NEW-SESSION-R27).
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

  // The trust question opens upward over the room above the box, so the box and the session controls stay where they
  // are (docs/design/COMPOSER.md). It is capped to that room, its explanation scrolling inside; where the room cannot
  // hold even its title, folder and answers, it takes its place in the column instead and the box moves down, since
  // a question whose folder is out of sight must never be answered.
  const screen = useRef<HTMLElement>(null);
  const above = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (asking === undefined) return;
    const fit = () => {
      const zone = above.current;
      const section = screen.current;
      const card = zone?.querySelector<HTMLElement>(".trust-question");
      const body = card?.querySelector<HTMLElement>(".trust-question-body");
      if (!zone || !section || !card || !body) return;
      // Measured over the room as it is with the question out of the column; a tight screen's scroll is kept.
      const scrolled = section.scrollTop;
      delete section.dataset.tight;
      card.style.maxHeight = "";
      const style = getComputedStyle(section);
      const top = parseFloat(style.paddingTop) || 0;
      const room = zone.getBoundingClientRect().top - section.getBoundingClientRect().top - top;
      // What must show: everything but the explanation, and its first line or so.
      const first = body.firstElementChild;
      const least = card.offsetHeight - body.offsetHeight + (first instanceof HTMLElement ? first.offsetHeight : 0);
      if (room >= least) {
        card.style.maxHeight = `${room}px`;
      } else {
        // In the column, capped to the screen's own height, so the title, folder and answers show together at its top.
        section.dataset.tight = "";
        // Measured before the scroll is restored, so from the column's top.
        const below = card.getBoundingClientRect().top - section.getBoundingClientRect().top;
        card.style.maxHeight = `${Math.max(least, section.clientHeight - below - (parseFloat(style.paddingBottom) || 0))}px`;
        section.scrollTop = scrolled;
      }
    };
    fit();
    // The room changes with the window and with the box and the session controls: text that grows the box, chips
    // that wrap. Only a change of height refits: neither the screen's nor the box column's height changes when the
    // question is capped, and the scroll bar a tight screen gains changes only widths, so fitting never sets this
    // off again. (jsdom has no ResizeObserver; the tests there need no layout.)
    const start = screen.current?.querySelector(".new-session-start");
    const heights = new Map<Element, number>();
    const watch =
      typeof ResizeObserver === "undefined"
        ? undefined
        : new ResizeObserver((entries) => {
            let changed = false;
            for (const entry of entries) {
              const height = entry.contentRect.height;
              if (heights.get(entry.target) !== height) changed = true;
              heights.set(entry.target, height);
            }
            if (changed) fit();
          });
    if (screen.current) watch?.observe(screen.current);
    if (start) watch?.observe(start);
    return () => {
      watch?.disconnect();
      if (screen.current) delete screen.current.dataset.tight;
    };
  }, [asking]);

  // The folder dialog is the local host's (FLOW-NEW-SESSION-R27, R19): a remote host's folders are not this computer's.
  const canAdd = onChooseFolder !== undefined && hostId === localHostId && !busy && asking === undefined;
  const addProject = async () => {
    if (!canAdd || choosing.current) return;
    choosing.current = true;
    try {
      const choice = await onChooseFolder();
      // A folder already a project is chosen without the question (R18); a new one is asked about first (R27).
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
  // Trusted, the project is added and chosen (FLOW-NEW-SESSION-R27); canceled, nothing changes (R17).
  const answer = async (trust: boolean) => {
    const path = asking;
    setAsking(undefined);
    backToWords();
    if (path === undefined || !trust || choosing.current) return;
    choosing.current = true;
    try {
      const project = await onAddProject(path);
      if (project) {
        setAdded(project);
        chooseProject(project.id);
      }
    } finally {
      choosing.current = false;
    }
  };

  // Ctrl+O chooses Add project from anywhere on the screen (FLOW-NEW-SESSION-R27). The listener is added once and
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

  // A git project's branches, read afresh each time the screen shows it, so a new session's base branch starts at the
  // repository's default branch (FLOW-NEW-SESSION-R21). The latest reader is used, so a new function each render
  // reads nothing again.
  const gitProject = project?.vcs === "git" ? project.id : undefined;
  const [read, setRead] = useState<{ projectId: string; branches: Branches }>();
  const lister = useRef(props.onListBranches);
  lister.current = props.onListBranches;
  const listing = props.onListBranches !== undefined;
  useEffect(() => {
    // A list read for this project before is not shown while it is read again: its branches may have changed.
    setRead(undefined);
    if (gitProject === undefined || !lister.current) return;
    let current = true;
    void lister.current(gitProject).then((branches) => {
      if (current) setRead({ projectId: gitProject, branches });
    });
    return () => {
      current = false;
    };
  }, [gitProject, listing]);
  const branches: Branches | undefined =
    gitProject === undefined || !listing ? undefined : read?.projectId === gitProject ? read.branches : { state: "reading" };

  const hostBackends = backends[hostId];
  // With no project yet, the where values are the local host's alone; the how values still show (R19).
  const chosen: SessionValues = project
    ? valuesFor(project, hostBackends ?? [], wanted)
    : { host_id: localHostId, project_id: "", workspace: "root", ...howFor(hostBackends ?? [], wanted) };
  // A new workspace starts from the base branch the chip shows: the one chosen, else the default (R21, R29).
  const base = chosen.workspace === "isolated" && branches?.state === "read" ? baseFor(branches.branches, wanted.base) : undefined;
  const values: SessionValues = base === undefined ? chosen : { ...chosen, base };

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
    <section ref={screen} className="new-session" aria-label="New session">
      <Wordmark />
      <div className="new-session-start">
        {/* In the status line's place above the box (docs/design/COMPOSER.md, The trust question), opening upward:
            the box and the session controls stay where they are. */}
        <div ref={above} className="new-session-above">
          {asking !== undefined && (
            <TrustQuestion
              name={folderName(asking)}
              path={asking}
              purpose="add"
              onTrust={() => void answer(true)}
              onDismiss={() => void answer(false)}
              takeFocus={props.initialAsking === undefined}
              forceCancelFocus={props.forceCancelFocus}
            />
          )}
          {asking === undefined && adding?.state === "failed" && (
            <p className="problem new-session-problem" role="alert">
              {adding.message}
            </p>
          )}
        </div>
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
          branches={branches}
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
