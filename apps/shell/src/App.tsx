import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { t } from "./catalog";
import "./App.css";
import { Button } from "./components/Button";
import type { AttachWhat } from "./components/AttachMenu";
import { Composer } from "./components/Composer";
import { RailFoot } from "./components/RailFoot";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { type FolderChoice, NewSessionScreen } from "./components/NewSessionScreen";
import { QuitQuestion } from "./components/QuitQuestion";
import { SessionControls } from "./components/SessionControls";
import { noneUnseen, unseenAfter } from "./session/unseen";
import { RailTop } from "./components/RailTop";
import { RailTree } from "./components/RailTree";
import { Shelf } from "./components/Shelf";
import { Transcript } from "./components/Transcript";
import { type CoreClient, describeError } from "./core/client";
import { type Connection, type CoreView, type ProjectsView, type SessionsView, connect } from "./core/connection";
import { type QuitAsk, type ShellControl, answerQuit, busySessions, cancelQuit } from "./core/shell";
import type { Project, Session, Workspace } from "./protocol/core-api.gen";
import { type BackendChoices, type Branches, type SessionValues, newestFirst, projectOrder, sessionShown } from "./session/controls";
import { contextUsed, tileOf } from "./session/tile";
import { type Answers, type Decision, answering } from "./session/answers";
import { type ShelfShown, shelfAfter, shelfFollowing, shelfOf, shelfTone, shownAfter, shownRequest } from "./session/shelf";
import { sharedSubscriptions } from "./session/subscriptions";
import { type Attached, attached, sendable, unreadable } from "./session/attachments";
import { type Follower, type SendProgress, type SessionView, type Sending, follow, progressOf } from "./session/follow";
import { initialStage, showsScreen, stage } from "./session/stage";
import { StartStop, readChoices, rememberPatch, remembered, startSession } from "./session/start";
import { mergeWorkspaces } from "./session/workspaces";

// The same objects every render, so effects and error-boundary keys that
// depend on a list see no change while Core's phase stays the same.
const UNAVAILABLE = { state: "unavailable" } as const;
const LOADING = { state: "loading" } as const;

/** How many `attachment.inspect` reads Core runs at once; it refuses more (PROTO-CORE-API, Sending). */
const INSPECTS_AT_ONCE = 8;

/** The projects part of the view, whatever phase Core is in. */
function projectsOf(view: CoreView): ProjectsView {
  switch (view.phase) {
    case "connected":
      return view.projects;
    case "lost":
    case "failed":
      return UNAVAILABLE;
    case "connecting":
      return LOADING;
  }
}

function sessionsOf(view: CoreView): SessionsView {
  switch (view.phase) {
    case "connected":
      return view.sessions;
    case "lost":
    case "failed":
      return UNAVAILABLE;
    case "connecting":
      return LOADING;
  }
}

/** A number that changes whenever `value` is a new object: a key that resets an error boundary on new data. */
function useVersion(value: unknown): string {
  const seen = useRef({ value, version: 0 });
  if (seen.current.value !== value) seen.current = { value, version: seen.current.version + 1 };
  return String(seen.current.version);
}

export function App({ client, shell }: { client: CoreClient; shell?: ShellControl }) {
  const [view, setView] = useState<CoreView>({ phase: "connecting" });
  // Bumped to attach again; the effect below reconnects on each change.
  const [attempt, setAttempt] = useState(0);
  const retry = useCallback(() => setAttempt((n) => n + 1), []);
  const connection = useRef<Connection | null>(null);

  // What the stage shows: the active session, or the new-session screen (session/stage.ts); and what the window
  // knows of the active session.
  const [{ activeId, composing, starting, firstSend }, dispatch] = useReducer(stage, initialStage);
  const [session, setSession] = useState<SessionView>();
  const follower = useRef<Follower | null>(null);

  // What the screen offers: each host's backends, and the values each project last started a session with.
  const [backends, setBackends] = useState<Record<string, BackendChoices[]>>({});
  const [lastUsed, setLastUsed] = useState<Record<string, SessionValues>>({});
  // Every project's workspaces, by id: the rail puts a session under its workspace's project, and the session
  // controls read a session's workspace from them. Each project's last read: null once read, else why it failed.
  const [workspaces, setWorkspaces] = useState<Record<string, Workspace>>({});
  const [workspaceReads, setWorkspaceReads] = useState<Record<string, string | null>>({});
  const [trusting, setTrusting] = useState<Record<string, Sending>>({});
  // The workspaces of each project whose trust the user withdrew here, as they were then. A session running
  // in one of them keeps the configuration it started with.
  const [withdrawn, setWithdrawn] = useState<Record<string, string[]>>({});
  const [adding, setAdding] = useState<Sending>();
  // The quit question, while it is open: how many sessions would stop, and whether Quit was chosen.
  const [quitAsk, setQuitAsk] = useState<QuitAsk & { quitting: boolean; failed?: string }>();
  const [renaming, setRenaming] = useState<Record<string, Sending>>({});
  // The session whose shelf is open, and its events, followed while it is open (FLOW-APPROVE-PERMISSION).
  const [shelfId, setShelfId] = useState<string>();
  const [shelfView, setShelfView] = useState<SessionView>();
  // The requests the open shelf has shown since it opened, which count toward its place (R09).
  const [shelfShown, setShelfShown] = useState<ShelfShown>();
  // The transcript and the shelf share Core's one subscription per session (session/subscriptions.ts).
  const shared = useMemo(() => sharedSubscriptions(client), [client]);
  const shelf = useMemo(() => shelfFollowing(shared, setShelfView), [shared]);
  // The user's answers to permission requests, on their way or refused, by session: the shelf and the transcript
  // read the same ones, so an answer sent from one is still on its way on the other (FLOW-APPROVE-PERMISSION-R18).
  const [answers, setAnswers] = useState<Answers>({});
  const sending = useMemo(() => answering(client, setAnswers), [client]);

  useEffect(() => {
    const running = connect(client, setView, undefined, (notification) => {
      // Where a send is (FLOW-SEND-MESSAGE-R16): the active session's, or the first message of a session starting.
      if (notification.method === "session.send_progress") {
        follower.current?.progress(notification.params);
        if (notification.params.session_id === startStop.current?.session) setStartProgress(progressOf(notification.params));
        return;
      }
      if (notification.method !== "session.event") return;
      // Each follower takes only its own session's events.
      follower.current?.receive(notification.params.event);
      shelf.receive(notification.params.event);
      sending.receive(notification.params.event);
    });
    connection.current = running;
    return () => running.stop();
  }, [client, shelf, sending, attempt]);

  // With no session chosen, show the newest: reopening the window shows the
  // session that was running when it closed.
  const sessions = sessionsOf(view);
  useEffect(() => {
    if (activeId === undefined && sessions.state === "ready") {
      const newest = newestFirst(sessions.sessions)[0];
      if (newest) dispatch({ type: "showNewest", id: newest.id });
    }
  }, [activeId, sessions]);

  // The unseen sessions (LAY-SHELL, Unseen), worked out afresh from each list Core reports and each change of the
  // active session. Kept in memory only: nothing survives the window closing.
  const shownId = composing ? undefined : activeId;
  const [unseen, setUnseen] = useState(noneUnseen);
  useEffect(() => {
    if (sessions.state === "ready") setUnseen((prev) => unseenAfter(prev, sessions.sessions, shownId));
  }, [sessions, shownId]);

  useEffect(() => {
    if (activeId === undefined) return;
    const following = follow(shared(), activeId, setSession);
    follower.current = following;
    dispatch({ type: "followed", id: activeId });
    return () => {
      following.stop();
      if (follower.current === following) follower.current = null;
    };
  }, [shared, activeId]);

  // Subscribe on each new connection, from where the transcript left off;
  // without one, the session is offline and answers wait.
  const link = view.phase === "connected" ? view.link : undefined;
  useEffect(() => {
    if (link !== undefined) follower.current?.attached();
    else follower.current?.detached();
  }, [link, activeId]);

  // The shelf is never open for the active session (FLOW-APPROVE-PERMISSION-R26): making its session active, by
  // any way, closes it at once, and so does its session having nothing left to ask.
  const openShelf = shelfId !== undefined && shelfId !== shownId ? shelfId : undefined;
  useEffect(() => {
    if (sessions.state === "ready") setShelfId((open) => shelfAfter(open, { type: "sessions", sessions: sessions.sessions, activeId: shownId }));
  }, [sessions, shownId]);
  const chevron = (id: string) => setShelfId((open) => shelfAfter(open, { type: "chevron", id, activeId: shownId }));
  const closeShelf = useCallback((id: string) => setShelfId((open) => shelfAfter(open, { type: "close", id })), []);
  // Its session is followed while the shelf is open, and no longer once it closes. The connection's changes come
  // first, so a shelf opened with a new connection subscribes on it once.
  useEffect(() => {
    if (link !== undefined) shelf.attached();
    else shelf.detached();
  }, [shelf, link]);
  const connected = link !== undefined;
  // A change of connection alone is the effect above's to handle.
  useEffect(() => shelf.follow(openShelf, connected), [shelf, openShelf]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => shelf.follow(undefined, false), [shelf]);

  // Quit from the tray icon: confirmed here first when sessions are busy (FLOW-CLOSE-AND-QUIT-R04). The
  // sessions are read afresh then; the rail's list stands in when Core cannot be asked.
  const knownSessions = useRef<Session[] | undefined>(undefined);
  knownSessions.current = sessions.state === "ready" ? sessions.sessions : undefined;
  useEffect(() => {
    if (!shell) return;
    return shell.onQuitRequested(() => {
      answerQuit(client, shell, knownSessions.current).then(
        // Asked again while open: the count is the fresh one, and Cancel puts the window back in the tray if it
        // was there before either Quit.
        (ask) => ask && setQuitAsk((open) => ({ ...ask, wasHidden: ask.wasHidden || (open?.wasHidden ?? false), quitting: false })),
        // Once the window has heard, the Shell waits on it to quit, so a failed answer is said, never dropped.
        (error) => {
          setQuitAsk((open) => ({
            busy: open ? open.busy : knownSessions.current && busySessions(knownSessions.current),
            wasHidden: open?.wasHidden ?? false,
            quitting: false,
            failed: `Ersa did not quit: ${describeError(error)}`,
          }));
          shell.show().catch(() => {});
        },
      );
    });
  }, [client, shell]);
  const quit = () => {
    setQuitAsk((open) => open && { ...open, quitting: true, failed: undefined });
    shell
      ?.quit()
      .catch((error) => setQuitAsk((open) => open && { ...open, quitting: false, failed: `Ersa did not quit: ${describeError(error)}` }));
  };
  const cancel = () => {
    if (quitAsk && shell) cancelQuit(shell, quitAsk).catch(() => {});
    setQuitAsk(undefined);
  };

  // The screen's choices, read again on each new connection: Core may have found a backend since.
  const hostId = view.phase === "connected" ? view.hello.host.id : undefined;
  useEffect(() => {
    if (link === undefined || hostId === undefined) return;
    let live = true;
    readChoices(client, hostId).then((choices) => {
      if (!live) return;
      // A host whose backends could not be read offers none; the screen says no backend is available.
      setBackends((b) => ({ ...b, [hostId]: choices.backends }));
      setLastUsed(choices.lastUsed);
    });
    return () => {
      live = false;
    };
  }, [client, link, hostId]);

  // Each host's name, by id, read on each new connection: the rail names a project's host when it is not this
  // machine. Best effort: a host not listed, or a read that failed or has not answered, is named by its id.
  const [hostNames, setHostNames] = useState<Record<string, string>>({});
  useEffect(() => {
    if (link === undefined) return;
    let live = true;
    client.call("host.list").then(
      (hosts) => live && setHostNames(Object.fromEntries(hosts.map((h) => [h.id, h.name]))),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [client, link]);

  // Each list's error boundary tries again whenever the list changes.
  const projects = projectsOf(view);
  const projectList = projects.state === "ready" ? projects.projects : undefined;
  // The workspaces sessions run in that no read has listed, once every project's read has answered: a session made
  // after the reads, or by another window, has the projects' workspaces read again, so its tile moves under its
  // project. A workspace no read lists (its project gone) keeps the same key, so it is read again only once.
  const allRead = projectList !== undefined && projectList.every((p) => p.id in workspaceReads);
  const unknownWorkspaces =
    allRead && sessions.state === "ready" ? [...new Set(sessions.sessions.map((s) => s.workspace_id).filter((id) => !(id in workspaces)))].sort().join(" ") : "";
  useEffect(() => {
    // Without a connection nothing has been read on the next one, so its sessions wait to be placed again.
    if (!projectList) {
      setWorkspaceReads({});
      return;
    }
    let live = true;
    // Each project's read is applied as it answers, so one slow host holds back only its own tiles; a session that
    // fits no project waits for every read, which the Shell gives up on after its request time limit. Merged
    // into what is known (session/workspaces.ts): a read that answers late never drops a workspace.
    for (const p of projectList) {
      client.call("workspace.list", { project_id: p.id }).then(
        (read) => {
          if (!live) return;
          setWorkspaces((known) => mergeWorkspaces(known, projectList, read));
          setWorkspaceReads((reads) => ({ ...reads, [p.id]: null }));
        },
        (error) => {
          if (!live) return;
          // What was known is kept, but not for a project no longer listed.
          setWorkspaces((known) => mergeWorkspaces(known, projectList, undefined));
          setWorkspaceReads((reads) => ({ ...reads, [p.id]: describeError(error) }));
        },
      );
    }
    return () => {
      live = false;
    };
    // unknownWorkspaces is not read here: a change to it is a reason to read again.
  }, [client, projectList, unknownWorkspaces]);
  const projectsVersion = useVersion(projects);
  const sessionsVersion = useVersion(sessions);
  const coreVersion = useVersion(view);
  const retrySubscribe = useCallback(() => follower.current?.attached(), []);

  // Stable for each session, so the transcript's memoised blocks do not re-render for it.
  const decide = useCallback((id: string, requestId: string, decision: Decision) => sending.decide(id, requestId, decision), [sending]);
  const decideShown = useMemo(() => (requestId: string, decision: Decision) => shownId !== undefined && decide(shownId, requestId, decision), [decide, shownId]);

  // New session shows the screen; leaving it, by choosing a session, leaves nothing behind (FLOW-NEW-SESSION-R03).
  const newSession = (project: Project) => dispatch({ type: "newSession", projectId: project.id });
  // The projects whose headings the user collapsed, kept while the window is open (LAY-SHELL, Collapsing).
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const toggleProject = (id: string) =>
    setCollapsed((was) => {
      const next = new Set(was);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const select = (id: string) => dispatch({ type: "select", id });

  // The first message starts the session; on a failure the screen says why and the composer keeps the message
  // (FLOW-NEW-SESSION-R08). Once started, it is the active session (R12).
  // Stop on the new-session screen reaches the start under way (FLOW-SEND-MESSAGE-R03).
  const startStop = useRef<StartStop>(undefined);
  const [startProgress, setStartProgress] = useState<SendProgress>();
  const start = async (text: string, values: SessionValues, attachments: readonly Attached[]) => {
    dispatch({ type: "sending" });
    setStartProgress(undefined);
    const stop = new StartStop(client);
    startStop.current = stop;
    const outcome = await startSession(client, values, text, stop, sendable(attachments)).finally(() => {
      if (startStop.current === stop) startStop.current = undefined;
    });
    if (!outcome.ok && "stopped" in outcome) {
      dispatch({ type: "stopped" });
      connection.current?.reload("sessions");
      return false;
    }
    if (!outcome.ok) {
      dispatch({ type: "failed", message: outcome.message });
      // The rail may already list the session the failed start made and removed.
      connection.current?.reload("sessions");
      return false;
    }
    setLastUsed((l) => ({ ...l, [values.project_id]: remembered(values) }));
    // Best effort: a failure only means the screen starts at these values in this window alone.
    client.call("settings.set", { patch: rememberPatch(values) }).catch(() => {});
    // Known at once, so the rail puts the new session's tile under its project as it appears (FLOW-NEW-SESSION-R13).
    setWorkspaces((known) => ({ ...known, [outcome.workspace.id]: outcome.workspace }));
    connection.current?.reload("sessions");
    dispatch({ type: "started", id: outcome.session.id, text, attachments, firstSend: outcome.firstSend });
    return true;
  };

  // The backend keeps the title (PROTO-CORE-API-R12); the list is read again to show it, in the rail and here.
  const rename = async (id: string, title: string) => {
    setRenaming((r) => ({ ...r, [id]: { state: "sending" } }));
    try {
      // Core answers with the title as the backend will list it.
      const renamed = await client.call("session.set_title", { id, title });
      setRenaming(({ [id]: _, ...rest }) => rest);
      connection.current?.renamed(id, renamed.title ?? title);
      return true;
    } catch (error) {
      setRenaming((r) => ({ ...r, [id]: { state: "failed", message: describeError(error) } }));
      return false;
    }
  };

  // Add project on the new-session screen (FLOW-NEW-SESSION-R17 to R19, R27): the Shell's folder dialog on this computer,
  // then `project.find`, so a folder that is already a project is chosen without the trust question. Without the
  // Shell, as in tests that give none, there is no dialog and Add project is unavailable.
  const pickFolder = shell?.pickFolder;
  const chooseFolder =
    pickFolder && view.phase === "connected"
      ? async (): Promise<FolderChoice> => {
          const host_id = view.hello.host.id;
          try {
            // A canceled dialog leaves everything as it was, an earlier failure's reason included (R17).
            const path = await pickFolder();
            if (path === null) return { kind: "canceled" };
            setAdding({ state: "sending" });
            const found = await client.call("project.find", { host_id, path });
            setAdding(undefined);
            return found ? { kind: "found", project: found } : { kind: "new", path };
          } catch (error) {
            setAdding({ state: "failed", message: describeError(error) });
            return { kind: "failed" };
          }
        }
      : undefined;

  // Attaching (FLOW-SEND-MESSAGE-R07, R13): the Shell's dialog on this computer, then `attachment.inspect` for each path
  // picked, and only those. A path Core can't read is attached marked with why, so the message waits until it is
  // removed (R10). Without the Shell, as in tests that give none, attach is unavailable.
  const pickFiles = shell?.pickFiles;
  // Reading the paths the user names, picked or dropped on the box (FLOW-SEND-MESSAGE-R08, R13): `attachment.inspect`
  // for each, and only those.
  const read =
    view.phase === "connected"
      ? async (paths: readonly string[], workspace: string | undefined): Promise<Attached[]> => {
          const host_id = view.hello.host.id;
          // At most INSPECTS_AT_ONCE in flight, the rest queued: Core reads that many at once and refuses more, which
          // would mark good paths unreadable. Kept in the order picked.
          const read: Attached[] = new Array(paths.length);
          let next = 0;
          const worker = async () => {
            for (let at = next++; at < paths.length; at = next++) {
              const path = paths[at] as string;
              // With the workspace, read on its own host, Core says whether sending copies it (FLOW-SEND-MESSAGE-R17); a new
              // workspace not made yet names the host.
              const params = workspace === undefined ? { host_id, path } : { path, workspace_id: workspace };
              read[at] = await client.call("attachment.inspect", params).then(
                (info) => attached(info, workspace),
                (error) => unreadable(path, describeError(error)),
              );
            }
          };
          await Promise.all(Array.from({ length: Math.min(INSPECTS_AT_ONCE, paths.length) }, worker));
          return read;
        }
      : undefined;
  const attach =
    pickFiles && pickFolder && read
      ? async (what: AttachWhat, workspace: string | undefined): Promise<Attached[]> => {
          let paths: string[];
          try {
            paths = what === "folder" ? [await pickFolder()].filter((p): p is string => p !== null) : await pickFiles(what === "images");
          } catch {
            return [];
          }
          return read(paths, workspace);
        }
      : undefined;

  // A git project's local branches, for the base branch a new workspace starts from (FLOW-NEW-SESSION-R21). Branches
  // that cannot be listed leave the base to Core, which starts the workspace at the default branch.
  const listBranches = (projectId: string): Promise<Branches> =>
    client.call("branch.list", { project_id: projectId }).then(
      (branches) => ({ state: "read", branches }),
      () => ({ state: "failed" }),
    );

  // Every project the Interface adds is trusted: Trust is the trust question's one answer that adds (FLOW-NEW-SESSION-R25).
  const addProject = async (path: string) => {
    if (view.phase !== "connected") return undefined;
    setAdding({ state: "sending" });
    try {
      const project = await client.call("project.add", { host_id: view.hello.host.id, path, trusted: true });
      setAdding(undefined);
      connection.current?.reload("projects");
      return project;
    } catch (error) {
      setAdding({ state: "failed", message: describeError(error) });
      return undefined;
    }
  };

  const setTrust = async (project: Project, trusted: boolean) => {
    setTrusting((s) => ({ ...s, [project.id]: { state: "sending" } }));
    try {
      await client.call("project.set_trust", { id: project.id, trusted });
      setTrusting(({ [project.id]: _, ...rest }) => rest);
      connection.current?.reload("projects");
      // Trust is read when a session starts, so withdrawing it leaves running sessions as they are. Best
      // effort: trust is already withdrawn, so a failed lookup only leaves the note out.
      if (!trusted) {
        const listed = await client.call("workspace.list", { project_id: project.id }).catch(() => []);
        setWithdrawn((w) => ({ ...w, [project.id]: listed.map((ws) => ws.id) }));
      }
    } catch (error) {
      setTrusting((s) => ({ ...s, [project.id]: { state: "failed", message: describeError(error) } }));
    }
  };

  // Said beneath an untrusted project while a session from before the withdrawal still runs; derived on each
  // render, so it goes once those sessions stop or the project is trusted again. A session's state arrives
  // with its tile (`session.tile`), so that is the state read.
  const keepingTrust: string[] = [];
  if (projects.state === "ready" && sessions.state === "ready") {
    const live = sessions.sessions.filter((s) => s.tile.state !== "ended" && s.tile.state !== "error");
    for (const p of projects.projects) {
      if (!p.trusted && live.some((s) => withdrawn[p.id]?.includes(s.workspace_id))) keepingTrust.push(p.id);
    }
  }
  // Why each project's workspaces could not be read; and whether some project's are still being read, in which
  // case a session the rail cannot place yet waits rather than sit at the end.
  const workspaceErrors: Record<string, string> = {};
  for (const [id, failure] of Object.entries(workspaceReads)) if (failure !== null) workspaceErrors[id] = failure;
  const placing = projectList !== undefined && !allRead;

  // The title as last listed, so the header keeps it while the list reloads
  // or Core is away.
  const titles = useRef<Record<string, string>>({});
  if (sessions.state === "ready") for (const s of sessions.sessions) if (s.title) titles.current[s.id] = s.title;
  const shown = !composing && session && session.id === activeId ? session : undefined;
  const firstSendHere = firstSend && firstSend.id === shown?.id ? firstSend : undefined;

  // The projects as last read, so a screen with a message being written stays while Core is away; sending then
  // fails with the reason, and the message stays in the composer.
  const knownProjects = useRef<Project[] | undefined>(undefined);
  const orderedBy = useRef<readonly Session[]>([]);
  if (projectList) knownProjects.current = projectList;
  const hello = view.phase === "connected" || view.phase === "lost" ? view.hello : undefined;
  const knownList = projectList ?? (showsScreen({ composing, starting, activeId }, false) ? knownProjects.current : undefined);
  // Most recently used first, by the sessions as last listed (FLOW-NEW-SESSION-R14).
  const order = projectOrder(knownList ?? [], sessions.state === "ready" ? sessions.sessions : undefined, orderedBy.current, workspaces);
  orderedBy.current = order.kept;
  const screenProjects = knownList && order.projects;

  // Each project's root workspace, for attachments on the new-session screen judged against the project folder.
  const rootWorkspaces: Record<string, string> = {};
  for (const w of Object.values(workspaces)) if (w.kind === "root") rootWorkspaces[w.project_id] = w.id;

  const projectById = new Map(projectList?.map((p) => [p.id, p]));

  // The session controls under the active session show its own values, as Core lists them (FLOW-NEW-SESSION-R12);
  // they change nothing during a session. Absent until the session's workspace and project are known.
  const listed = sessions.state === "ready" ? sessions.sessions.find((s) => s.id === shown?.id) : undefined;
  // Ersa can't reach the sessions: the composer says so and is unavailable until it does (FLOW-SEND-MESSAGE-R06).
  const away = view.phase === "connected" ? undefined : view.phase === "failed" ? "unreachable" : "reconnecting";
  const listedWorkspace = listed && workspaces[listed.workspace_id];
  const listedProject = listedWorkspace && projectById.get(listedWorkspace.project_id);
  const sessionControls = listed && listedWorkspace && sessionShown(listed, listedWorkspace, backends[listed.host_id] ?? []);
  const used = listed && contextUsed(listed.tile);

  // The open shelf as the rail draws it: the session's oldest request, with its readouts as its tile shows them.
  const shelfSession = openShelf !== undefined && sessions.state === "ready" ? sessions.sessions.find((s) => s.id === openShelf) : undefined;
  const shelfShows =
    shelfSession &&
    shelfOf(
      shelfView?.id === shelfSession.id ? shelfView : undefined,
      shelfSession.tile.pending,
      shelfShown?.id === shelfSession.id ? shelfShown.requests : undefined,
    );
  // Derived as the shelf renders, so the count is right on the render that shows the next request.
  const nextShown = shownAfter(shelfShown, shelfSession?.id, shelfShows && shownRequest(shelfShows));
  if (nextShown !== shelfShown) setShelfShown(nextShown);
  const shelfOpen = (() => {
    if (!shelfSession || !shelfShows) return undefined;
    const tile = tileOf(shelfSession, backends);
    return {
      id: shelfSession.id,
      tone: shelfTone(shelfShows),
      render: (domId: string) => (
        <Shelf
          id={domId}
          tile={tile}
          shows={shelfShows}
          onRetry={shelf.attached}
          answers={answers[shelfSession.id]}
          live={connected}
          onDecide={(requestId, decision) => decide(shelfSession.id, requestId, decision)}
        />
      ),
    };
  })();

  return (
    <div className="app">
      <nav className="rail" aria-label="Rail">
        {/* Unavailable until Ersa has connected (LAY-SHELL, The foot). */}
        <RailTop onNewSession={() => dispatch({ type: "newSession" })} disabled={view.phase !== "connected"} />
        <div className="rail-body">
          <ErrorBoundary what={t("rail.tree.boundaryName")} resetKey={`${projectsVersion}:${sessionsVersion}`}>
            <RailTree
              projects={projects}
              sessions={sessions}
              workspaces={workspaces}
              workspaceErrors={workspaceErrors}
              placing={placing}
              activeId={shownId}
              onSelect={select}
              onNewSession={newSession}
              collapsed={collapsed}
              onToggle={toggleProject}
              onSetTrust={setTrust}
              trusting={trusting}
              keepingTrust={keepingTrust}
              localHost={hello?.host.id}
              hostNames={hostNames}
              backends={backends}
              unseen={unseen.ids}
              shelf={shelfOpen}
              onChevron={chevron}
              onCloseShelf={closeShelf}
            />
          </ErrorBoundary>
        </div>
        <div className="rail-core">
          {quitAsk && (
            <QuitQuestion busy={quitAsk.busy} quitting={quitAsk.quitting} failed={quitAsk.failed} onQuit={quit} onCancel={cancel} />
          )}
          <ErrorBoundary what={t("rail.foot.boundaryName")} resetKey={coreVersion} action={<Button onClick={retry}>{t("rail.foot.retryButton")}</Button>}>
            <RailFoot view={view} onRetry={retry} />
          </ErrorBoundary>
        </div>
      </nav>
      {shown ? (
        <main className="stage">
          {/* Tries again on each new event, and keeps the composer out of it. */}
          <ErrorBoundary what="This session" resetKey={`${shown.id}:${shown.transcript.lastSeq}`}>
            <Transcript
              key={shown.id}
              title={(activeId && titles.current[activeId]) || t("rail.tile.untitledLabel")}
              untitled={!(activeId && titles.current[activeId])}
              view={shown}
              answers={answers[shown.id]}
              onDecide={decideShown}
              onRetry={retrySubscribe}
              onRename={(title) => rename(shown.id, title)}
              renaming={renaming[shown.id]}
              // A refusal goes; a rename still on its way stays, so its answer is shown.
              onRenameReset={() =>
                setRenaming(({ [shown.id]: was, ...rest }) => (was?.state === "sending" ? { ...rest, [shown.id]: was } : rest))
              }
              renameDisabled={view.phase !== "connected"}
            />
          </ErrorBoundary>
          <ErrorBoundary what="The composer" resetKey={shown.id}>
            <Composer
              key={shown.id}
              unavailable={away}
              // An open permission request holds the focal glow; the box shows focus by its edge alone.
              quietFocus={listed?.tile.pending.some((p) => p.kind === "permission")}
              // A kept session's first message that did not go through: why, and the text when it is to go again.
              sending={shown.message ?? firstSendHere?.sending}
              initialText={firstSendHere?.text}
              initialAttachments={firstSendHere?.attachments}
              onAttach={attach && ((what) => attach(what, listed?.workspace_id))}
              onRead={read && ((paths) => read(paths, listed?.workspace_id))}
              dragDrop={shell?.onDragDrop}
              workspace={listed?.workspace_id}
              onSend={(text, attachments) => follower.current?.send(text, sendable(attachments)) ?? Promise.resolve(false)}
              // Stop, from sending until the turn ends: cancels the message on its way, or interrupts the turn (FLOW-SEND-MESSAGE-R03).
              working={shown.transcript.working || shown.turnPending}
              onStop={() => void follower.current?.interrupt()}
              controls={
                sessionControls &&
                listedProject &&
                hello && (
                  <SessionControls
                  hosts={[hello.host]}
                  localHostId={hello.host.id}
                  projects={[listedProject]}
                  backends={[sessionControls.backend]}
                  values={sessionControls.values}
                  workspaceLabel={sessionControls.workspaceLabel}
                  onChoose={() => {}}
                  readOnly
                  used={used}
                  />
                )
              }
            />
          </ErrorBoundary>
        </main>
      ) : hello && screenProjects && showsScreen({ composing, starting, activeId }, view.phase === "connected") ? (
        <main className="stage">
          <ErrorBoundary what="The new-session screen" resetKey={composing?.projectId ?? ""}>
            <NewSessionScreen
              // Drawn afresh on each New session, so its composer takes focus (FLOW-NEW-SESSION-R01).
              key={composing?.key ?? 0}
              hosts={[hello.host]}
              localHostId={hello.host.id}
              projects={screenProjects}
              backends={backends}
              lastUsed={lastUsed}
              projectId={composing?.projectId}
              onStart={start}
              onAttach={attach}
              onRead={read}
              dragDrop={shell?.onDragDrop}
              rootWorkspaces={rootWorkspaces}
              starting={starting?.state === "sending" ? { state: "sending", progress: startProgress } : starting}
              onChooseFolder={chooseFolder}
              onListBranches={listBranches}
              onAddProject={addProject}
              away={away}
              onStop={() => startStop.current?.stop()}
              adding={adding}
            />
          </ErrorBoundary>
        </main>
      ) : (
        <main className="stage stage-empty">No session open. Start one from a project in the rail.</main>
      )}
    </div>
  );
}
