import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { t } from "../catalog";
import type { BackendInfo, Project } from "../protocol/core-api.gen";
import {
  type SessionValues,
  available,
  howFor,
  seedScreen,
  startingProject,
  valuesFor,
} from "../session/controls";
import type { ShellControl } from "../core/shell";
import { type Attached, pathName } from "../session/attachments";
import type { Sending } from "../session/follow";
import type { AttachWhat } from "./AttachMenu";
import { Composer, type Unavailable } from "./Composer";
import { ContextRing } from "./ContextRing";
import { type ControlKey, DEFAULT_EFFORT, type Host, SessionControls } from "./SessionControls";
import { TrustQuestion } from "./TrustQuestion";
import "./NewSessionScreen.css";

/**
 * What came of the folder dialog: the user canceled it, picked a folder that is already a project on the host
 * (`project.find`, FLOW-NEW-SESSION-R18), picked one that is not yet, or it failed, with the reason in `adding`.
 */
/**
 * A folder the user picked, on its way to being a project: Ersa finding whether it is one already (`project.find`),
 * adding it after Trust (`project.add`), or why either failed.
 */
export type Adding = { state: "finding" } | { state: "adding" } | { state: "failed"; message: string };

export type FolderChoice = { kind: "canceled" } | { kind: "found"; project: Project } | { kind: "new"; path: string } | { kind: "failed" };

interface NewSessionScreenProps {
  hosts: readonly Host[];
  /** The host Ersa runs on: its chip reads Local, and the screen opened for no project starts at its project. */
  localHostId: string;
  /** Most recently used first (FLOW-NEW-SESSION-R14). */
  projects: readonly Project[];
  /** Each host's backends from `backend.list`, by host id; a host absent here is still being detected. */
  backends: Readonly<Record<string, readonly BackendInfo[]>>;
  /** Why a host's backends could not be listed, by host id: the screen says detection failed (FLOW-NEW-SESSION-R11). */
  backendFailures?: Readonly<Record<string, string>>;
  /** The values the user last started a session with, by project id. */
  lastUsed?: Readonly<Record<string, SessionValues>>;
  /** The project the user chose New session for; else the local host's most recently used (FLOW-NEW-SESSION-R15). */
  projectId?: string;
  /** Starts the session with the first message; resolves true once it has started. */
  onStart: (text: string, values: SessionValues, attachments: readonly Attached[]) => Promise<boolean>;
  /** Opens the system's dialog for files, images or a folder and reads each path picked (FLOW-SEND-MESSAGE-R07); paths dropped on the box go to `onRead`. */
  onAttach?: (what: AttachWhat, workspace: string | undefined) => Promise<Attached[]>;
  /** Reads each path dropped on the composer's box (FLOW-SEND-MESSAGE-R08). */
  onRead?: (paths: readonly string[], workspace: string | undefined) => Promise<Attached[]>;
  /** Hears drags from the file manager over the window, for the composer's box (FLOW-SEND-MESSAGE-R08). */
  dragDrop?: ShellControl["onDragDrop"];
  /** Each project's root workspace, by project id: the workspace a session in the project folder runs in. */
  rootWorkspaces?: Readonly<Record<string, string>>;
  starting?: Sending;
  /**
   * Opens the system's folder dialog on the local host and finds whether the folder picked is already a project
   * there; absent while there is no dialog to open or while something stands over the screen (the sessions page, the
   * quit or delete question), and Add project and Ctrl+O then do nothing.
   */
  onChooseFolder?: () => Promise<FolderChoice>;
  /** Adds the folder as a trusted project on the local host (FLOW-NEW-SESSION-R25); resolves to the project once Core added it. */
  onAddProject: (path: string) => Promise<Project | undefined>;
  /** Stop while the first message is on its way, as while the new session's backend starts (FLOW-SEND-MESSAGE-R03). */
  onStop?: () => void;
  /** Ersa can't reach the sessions: trying again, or no longer trying (FLOW-SEND-MESSAGE-R06). */
  away?: Extract<Unavailable, "reconnecting" | "unreachable">;
  /** Finding or adding a picked folder, or why it failed. */
  adding?: Adding;
  /**
   * The user moved past a failed add, by choosing another project or host or by sending (FLOW-NEW-SESSION-R44): App
   * drops the failure, which it keeps across New session.
   */
  onPassAddFailure?: () => void;
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
  const { hosts, localHostId, backends, backendFailures = {}, lastUsed = {}, onStart, starting, onChooseFolder, onAddProject, adding } = props;
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
  const addFailed = adding?.state === "failed" ? adding : undefined;
  // Finding or adding a picked folder: nothing is sent meanwhile (FLOW-NEW-SESSION-R42), and the screen says so in the
  // status line's place with the ring for work of unknown length, unless it says Ersa can't reach the sessions,
  // which comes first (docs/design/COMPOSER.md, The status line).
  const folderBusy = adding?.state === "finding" || adding?.state === "adding";
  const folderWait = folderBusy && props.away === undefined ? adding.state : undefined;
  // While the dialog is open, or a folder is being found or added, another Add project does nothing.
  const choosing = useRef(false);
  const words = useRef<HTMLTextAreaElement>(null);
  const backToWords = () => words.current?.focus({ preventScroll: true });
  // A project starts at what it last used, except the how values the user chose on this screen (FLOW-NEW-SESSION-R02).
  const chooseProject = (id: string) =>
    setSeed((s) => ({ ...s, touched: true, wanted: { ...lastUsed[id], ...s.picked, project_id: id } }));
  // A how value chosen here, with the backend it was chosen for; a backend chosen anew drops the rest.
  const pickHow = (key: "backend" | "model" | "effort" | "permission_mode", next: Partial<SessionValues>) =>
    setSeed((s) => ({
      ...s,
      touched: true,
      wanted: next,
      picked: key === "backend" ? { backend: next.backend } : { ...s.picked, backend: next.backend, [key]: next[key] },
    }));

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
      const style = getComputedStyle(section);
      const top = parseFloat(style.paddingTop) || 0;
      const room = zone.getBoundingClientRect().top - section.getBoundingClientRect().top - top;
      const path = card.querySelector<HTMLElement>(".trust-question-path");
      // Uncapped, at the width the card has now. What must show over the room above the box: the title, the folder,
      // the answers and the explanation's first paragraph. In the column, where the cap is the screen's height, a long
      // folder may give up all but its first line and scroll inside itself, so the title, the folder and the answers
      // still show together.
      const measure = () => {
        card.style.maxHeight = "";
        body.style.minHeight = "";
        if (path) path.style.minHeight = "";
        const first = body.firstElementChild;
        const firstHeight = first instanceof HTMLElement ? first.offsetHeight : 0;
        const pathHeight = path?.offsetHeight ?? 0;
        const pathLine = path ? Math.min(pathHeight, parseFloat(getComputedStyle(path).lineHeight) || pathHeight) : 0;
        const rest = card.offsetHeight - body.offsetHeight - pathHeight;
        return { firstHeight, pathHeight, rest, whole: rest + pathHeight + firstHeight, least: rest + pathLine + firstHeight };
      };
      // Capped: the explanation keeps its first paragraph, and the folder keeps every line the cap leaves room for,
      // so the explanation gives up its room before the folder gives up a line.
      const cap = (to: number, sizes: ReturnType<typeof measure>) => {
        card.style.maxHeight = `${to}px`;
        body.style.minHeight = `${sizes.firstHeight}px`;
        if (path) path.style.minHeight = `${Math.max(0, Math.min(sizes.pathHeight, to - sizes.rest - sizes.firstHeight))}px`;
      };
      const loose = measure();
      if (room >= loose.whole) {
        cap(room, loose);
      } else {
        // In the column, capped to the screen's own height, so the title, folder and answers show together at its top.
        // Measured again there, since the scroll bar a tight screen gains narrows the card and may wrap the folder.
        section.dataset.tight = "";
        const tight = measure();
        // Measured before the scroll is restored, so from the column's top; and from the room the card stands in, not
        // the card, which its entrance still moves.
        const below = zone.getBoundingClientRect().top - section.getBoundingClientRect().top;
        cap(Math.max(tight.least, section.clientHeight - below - (parseFloat(style.paddingBottom) || 0)), tight);
        section.scrollTop = scrolled;
      }
    };
    fit();
    // The room changes with the window and with the box and the session controls: text that grows the box, chips
    // that wrap, a narrower window that wraps a long folder onto more lines. Fitting never sets this off again for
    // good: it always measures with the question out of the column, so its answer depends on the window alone and
    // not on the scroll bar a tight screen gains. (jsdom has no ResizeObserver; the tests there need no layout.)
    const start = screen.current?.querySelector(".new-session-start");
    const sizes = new Map<Element, string>();
    const watch =
      typeof ResizeObserver === "undefined"
        ? undefined
        : new ResizeObserver((entries) => {
            let changed = false;
            for (const entry of entries) {
              const size = `${entry.contentRect.width}x${entry.contentRect.height}`;
              if (sizes.get(entry.target) !== size) changed = true;
              sizes.set(entry.target, size);
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

  // Ctrl+O chooses Add project from anywhere on the screen (FLOW-NEW-SESSION-R27); App gives no `onChooseFolder`
  // while something stands over the screen, so it then does nothing. While the trust question is open, Escape cancels
  // it from anywhere on the screen, the message box included (docs/design/COMPOSER.md). The listener is added once
  // and reads the latest state through a ref.
  const keys = useRef({ addProject, cancel: () => {}, asking: false });
  keys.current = { addProject, cancel: () => void answer(false), asking: asking !== undefined };
  useEffect(() => {
    // From the screen, or from nowhere in particular: never from a question standing over it, nor while the screen is
    // out of reach behind one or behind the sessions page.
    const onScreen = (target: EventTarget | null) => {
      const section = screen.current;
      if (!section || section.closest("[inert], [hidden]")) return false;
      return target === document.body || (target instanceof Node && section.contains(target));
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey && event.key.toLowerCase() === "o") {
        // The keys are Add project's on this screen, never the webview's own.
        event.preventDefault();
        void keys.current.addProject();
      } else if (event.key === "Escape" && keys.current.asking && onScreen(event.target)) {
        event.preventDefault();
        keys.current.cancel();
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
  // Attachments are judged against the project folder's root workspace when it is chosen, and against none for a new
  // workspace, which is not made yet and holds nothing (FLOW-SEND-MESSAGE-R17).
  const attachTo = project && values.workspace === "root" ? props.rootWorkspaces?.[project.id] : undefined;

  const onChoose = (key: ControlKey, id: string) => {
    // Another project or host chosen moves past a failed add: its reason no longer applies (FLOW-NEW-SESSION-R44).
    if (addFailed && (key === "host" || key === "project")) props.onPassAddFailure?.();
    if (key === "host") {
      // A host's own projects follow it; its most recently used one, at what was last used there.
      const next = startingProject(projects, id);
      if (next) chooseProject(next.id);
    } else if (key === "project") {
      chooseProject(id);
    } else if (key === "backend") {
      // A model or effort chosen for one backend means nothing to another.
      pickHow(key, { ...values, backend: id, model: undefined, effort: undefined, permission_mode: undefined });
    } else if (key === "effort") {
      // The Default row chooses no level: the backend's own effort (docs/design/COMPOSER.md).
      pickHow(key, { ...values, effort: id === DEFAULT_EFFORT ? undefined : id });
    } else if (key === "model" || key === "permission_mode") {
      pickHow(key, { ...values, [key]: id });
    } else {
      // The Worktree checkbox: checked, a new isolated workspace; unchecked, the project folder.
      setWanted({ ...values, workspace: id === "isolated" ? "isolated" : "root" });
    }
  };

  // The box is unavailable while Ersa can't reach the sessions, while the host's backends are being detected
  // (FLOW-NEW-SESSION-R23), and while the host has no backend to run.
  // With none to run, the line says why the host could not list them (R11), else names a backend that failed
  // detection, else says none is installed (R07).
  const failed = hostBackends?.find((b) => b.status === "error");
  const listingFailed = backendFailures[hostId];
  const unavailable: Unavailable | undefined =
    props.away ??
    (hostBackends === undefined
      ? "detecting"
      : available(hostBackends).length > 0
        ? undefined
        : listingFailed !== undefined
          ? { detectionFailed: listingFailed }
          : failed
            ? { failedBackend: failed.label }
            : "no-backend");

  return (
    <section ref={screen} className="new-session" aria-label="New session">
      <Wordmark />
      <div className="new-session-start">
        {/* In the status line's place above the box (docs/design/COMPOSER.md, The trust question), opening upward:
            the box and the session controls stay where they are. */}
        <div ref={above} className="new-session-above">
          {asking !== undefined && (
            <TrustQuestion
              name={pathName(asking)}
              path={asking}
              purpose="add"
              onTrust={() => void answer(true)}
              onDismiss={() => void answer(false)}
              takeFocus={props.initialAsking === undefined}
              forceCancelFocus={props.forceCancelFocus}
            />
          )}
          {folderWait && (
            <p className="composer-status-line" role="status">
              <ContextRing turning />
              {folderWait === "finding" ? t("composer.findingFolderProgress") : t("composer.addingProjectProgress")}
            </p>
          )}
          {asking === undefined && addFailed && (
            <p className="problem new-session-problem" role="alert">
              {addFailed.message}
            </p>
          )}
        </div>
        <Composer
          inputRef={words}
          // The trust question, or why a folder could not be added, opens in the status line's place, and the
          // question holds the focal glow while it is open (docs/design/COMPOSER.md, The trust question).
          statusTaken={asking !== undefined || addFailed !== undefined || folderWait !== undefined}
          // Nothing is sent while a picked folder is found, asked about or added, so no session starts in the project
          // chosen before it (FLOW-NEW-SESSION-R42); the words stay the user's to edit. A failed add frees send again.
          held={asking !== undefined || folderBusy}
          quietFocus={asking !== undefined}
          autoFocus={props.autoFocus ?? true}
          force={props.forceFocus ? "focus" : undefined}
          initialText={props.initialText}
          unavailable={unavailable}
          sending={starting}
          onStop={props.onStop}
          onWrite={hold}
          onAttach={props.onAttach && ((what) => props.onAttach?.(what, attachTo) ?? Promise.resolve([]))}
          onRead={props.onRead && ((paths) => props.onRead?.(paths, attachTo) ?? Promise.resolve([]))}
          dragDrop={props.dragDrop}
          workspace={attachTo}
          onSend={(text, attachments) => {
            hold();
            // A send moves past a failed add, whose reason holds the status line's place where the send has its own
            // progress or failure to show (FLOW-NEW-SESSION-R44).
            if (addFailed) props.onPassAddFailure?.();
            // With no project, sending opens the folder dialog and the message stays for the user to send (R19).
            if (!project) {
              void addProject();
              return Promise.resolve(false);
            }
            return onStart(text, values, attachments);
          }}
        />
        <SessionControls
          hosts={hosts}
          localHostId={localHostId}
          projects={projects}
          backends={hostBackends}
          detectionFailed={listingFailed !== undefined}
          values={values}
          onChoose={onChoose}
          onAddProject={canAdd ? () => void addProject() : undefined}
          disabled={busy || folderBusy}
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
