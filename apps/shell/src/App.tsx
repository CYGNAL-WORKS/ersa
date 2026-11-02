import { useCallback, useEffect, useRef, useState } from "react";
import "./App.css";
import { AddProject } from "./components/AddProject";
import { Button } from "./components/Button";
import { Composer } from "./components/Composer";
import { CoreStatus } from "./components/CoreStatus";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { NewSessionScreen } from "./components/NewSessionScreen";
import { ProjectList } from "./components/ProjectList";
import { QuitQuestion } from "./components/QuitQuestion";
import { SessionControls } from "./components/SessionControls";
import { SessionList, type WorkspaceName, newestFirst } from "./components/SessionList";
import { Transcript } from "./components/Transcript";
import { type CoreClient, describeError } from "./core/client";
import { type Connection, type CoreView, type ProjectsView, type SessionsView, connect } from "./core/connection";
import { type QuitAsk, type ShellControl, answerQuit, busySessions, cancelQuit } from "./core/shell";
import type { Project, Session, Workspace } from "./protocol/core-api.gen";
import { type BackendChoices, type SessionValues, valuesFor } from "./session/controls";
import { type Follower, type SessionView, type Sending, follow } from "./session/follow";
import { readChoices, rememberPatch, startSession } from "./session/start";

// The same objects every render, so effects and error-boundary keys that
// depend on a list see no change while Core's phase stays the same.
const UNAVAILABLE = { state: "unavailable" } as const;
const LOADING = { state: "loading" } as const;

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

  // The session the stage shows, and what the window knows of it.
  const [activeId, setActiveId] = useState<string>();
  const [session, setSession] = useState<SessionView>();
  const follower = useRef<Follower | null>(null);

  // The new-session screen, while the user has chosen New session (for a project, or none yet); with no session
  // to show, the stage shows it too (LAY-SHELL). Nothing is created until its first message (FLOW-NEW-SESSION-R03).
  const [composing, setComposing] = useState<{ projectId?: string }>();
  const [starting, setStarting] = useState<Sending>();
  // The session just started, until the window follows it: the screen stays, sending, so the stage never passes
  // through the empty state between the screen and the session's transcript.
  const [opening, setOpening] = useState<string>();
  // What the screen offers: each host's backends, and the values each project last started a session with.
  const [backends, setBackends] = useState<Record<string, BackendChoices[]>>({});
  const [lastUsed, setLastUsed] = useState<Record<string, SessionValues>>({});
  // Every project's workspaces, by id: the rail names them above their sessions, and the session controls read a
  // session's workspace from them. Bumped to read them again after a session starts in a new one.
  const [workspaces, setWorkspaces] = useState<Record<string, Workspace>>({});
  const [workspacesRead, setWorkspacesRead] = useState(0);
  const [trusting, setTrusting] = useState<Record<string, Sending>>({});
  // The workspaces of each project whose trust the user withdrew here, as they were then. A session running
  // in one of them keeps the configuration it started with.
  const [withdrawn, setWithdrawn] = useState<Record<string, string[]>>({});
  const [adding, setAdding] = useState<Sending>();
  // The quit question, while it is open: how many sessions would stop, and whether Quit was chosen.
  const [quitAsk, setQuitAsk] = useState<QuitAsk & { quitting: boolean; failed?: string }>();
  const [renaming, setRenaming] = useState<Record<string, Sending>>({});

  useEffect(() => {
    const running = connect(client, setView, undefined, (notification) => {
      if (notification.method === "session.event") follower.current?.receive(notification.params.event);
    });
    connection.current = running;
    return () => running.stop();
  }, [client, attempt]);

  // With no session chosen, show the newest: reopening the window shows the
  // session that was running when it closed.
  const sessions = sessionsOf(view);
  useEffect(() => {
    if (activeId === undefined && sessions.state === "ready") {
      const newest = newestFirst(sessions.sessions)[0];
      if (newest) setActiveId(newest.id);
    }
  }, [activeId, sessions]);

  useEffect(() => {
    if (activeId === undefined) return;
    const following = follow(client, activeId, setSession);
    follower.current = following;
    return () => {
      following.stop();
      if (follower.current === following) follower.current = null;
    };
  }, [client, activeId]);

  // Subscribe on each new connection, from where the transcript left off;
  // without one, the session is offline and answers wait.
  const link = view.phase === "connected" ? view.link : undefined;
  useEffect(() => {
    if (link !== undefined) follower.current?.attached();
    else follower.current?.detached();
  }, [link, activeId]);

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
        // The Shell no longer quits by itself once the window has heard, so a failed answer is said, never dropped.
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

  // Each list's error boundary tries again whenever the list changes.
  const projects = projectsOf(view);
  const projectList = projects.state === "ready" ? projects.projects : undefined;
  useEffect(() => {
    if (!projectList) return;
    let live = true;
    // A project whose read fails keeps the workspaces last read for it, so its sessions keep their names and
    // controls; a project never read leaves its sessions in the rail without them.
    Promise.all(projectList.map((p) => client.call("workspace.list", { project_id: p.id }).then((list) => [p.id, list] as const, () => undefined))).then(
      (reads) => {
        if (!live) return;
        setWorkspaces((known) => {
          // Kept: the workspaces of a listed project whose read failed; a project no longer listed takes its own.
          const failed = new Set(projectList.filter((_, i) => reads[i] === undefined).map((p) => p.id));
          const kept = Object.values(known).filter((w) => failed.has(w.project_id));
          const read = reads.flatMap((r) => (r ? r[1] : []));
          return Object.fromEntries([...kept, ...read].map((w) => [w.id, w]));
        });
      },
    );
    return () => {
      live = false;
    };
  }, [client, projectList, workspacesRead]);
  const projectsVersion = useVersion(projects);
  const sessionsVersion = useVersion(sessions);
  const coreVersion = useVersion(view);
  const retrySubscribe = useCallback(() => follower.current?.attached(), []);

  // Stable, so the transcript's memoised blocks do not re-render for it.
  const decide = useCallback((requestId: string, behavior: "allow" | "deny") => follower.current?.decide(requestId, behavior), []);

  // New session shows the screen; leaving it, by choosing a session, leaves nothing behind (FLOW-NEW-SESSION-R03).
  const newSession = (project: Project) => {
    if (starting?.state !== "sending") setStarting(undefined);
    setComposing({ projectId: project.id });
  };
  const select = (id: string) => {
    if (starting?.state === "sending") return;
    setComposing(undefined);
    setStarting(undefined);
    setActiveId(id);
  };

  // The first message starts the session; on a failure the screen says why and the composer keeps the message
  // (FLOW-NEW-SESSION-R08). Once started, it is the active session (R09).
  const start = async (text: string, values: SessionValues) => {
    setStarting({ state: "sending" });
    const outcome = await startSession(client, values, text);
    if (!outcome.ok) {
      setStarting({ state: "failed", message: outcome.message });
      return false;
    }
    setLastUsed((l) => ({ ...l, [values.project_id]: values }));
    // Best effort: a failure only means the screen starts at these values in this window alone.
    client.call("settings.set", { patch: rememberPatch(values) }).catch(() => {});
    connection.current?.reload("sessions");
    if (values.workspace === "isolated") setWorkspacesRead((n) => n + 1);
    setOpening(outcome.session.id);
    setActiveId(outcome.session.id);
    return true;
  };
  useEffect(() => {
    if (opening !== undefined && session?.id === opening) {
      setOpening(undefined);
      setComposing(undefined);
      setStarting(undefined);
    }
  }, [opening, session]);

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

  const addProject = async (path: string, trusted: boolean) => {
    if (view.phase !== "connected") return false;
    setAdding({ state: "sending" });
    try {
      await client.call("project.add", { host_id: view.hello.host.id, path, trusted });
      setAdding(undefined);
      connection.current?.reload("projects");
      return true;
    } catch (error) {
      setAdding({ state: "failed", message: describeError(error) });
      return false;
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
  const trustNotes: Record<string, string> = {};
  if (projects.state === "ready" && sessions.state === "ready") {
    const live = sessions.sessions.filter((s) => s.tile.state !== "ended" && s.tile.state !== "error");
    for (const p of projects.projects) {
      if (!p.trusted && live.some((s) => withdrawn[p.id]?.includes(s.workspace_id))) {
        trustNotes[p.id] = "Running sessions keep their current settings until they are stopped.";
      }
    }
  }

  // The title as last listed, so the header keeps it while the list reloads
  // or Core is away.
  const titles = useRef<Record<string, string>>({});
  if (sessions.state === "ready") for (const s of sessions.sessions) if (s.title) titles.current[s.id] = s.title;
  const shown = !composing && session && session.id === activeId ? session : undefined;

  // The projects as last read, so a screen with a message being written stays while Core is away; sending then
  // fails with the reason, and the message stays in the composer.
  const knownProjects = useRef<Project[] | undefined>(undefined);
  if (projectList) knownProjects.current = projectList;
  const hello = view.phase === "connected" || view.phase === "lost" ? view.hello : undefined;
  const screenProjects = projectList ?? (composing ? knownProjects.current : undefined);

  // The rail's name for each workspace: its project, and its branch or the root (LAY-SHELL).
  const projectById = new Map(projectList?.map((p) => [p.id, p]));
  const workspaceNames: Record<string, WorkspaceName> = {};
  for (const w of Object.values(workspaces)) {
    const project = projectById.get(w.project_id);
    if (project) workspaceNames[w.id] = { project: project.name, name: w.kind === "root" ? "Root workspace" : w.vcs_ref || w.path };
  }

  // The session controls under the active session show its values (FLOW-NEW-SESSION-R09); they change nothing
  // during a session. Absent until the session's workspace and project are known.
  const listed = sessions.state === "ready" ? sessions.sessions.find((s) => s.id === shown?.id) : undefined;
  const listedWorkspace = listed && workspaces[listed.workspace_id];
  const listedProject = listedWorkspace && projectById.get(listedWorkspace.project_id);
  const sessionValues =
    listed &&
    listedWorkspace &&
    listedProject &&
    valuesFor(listedProject, backends[listed.host_id] ?? [], {
      workspace: listedWorkspace.kind,
      backend: listed.backend,
      model: listed.model,
      effort: listed.effort,
      permission_mode: listed.permission_mode,
    });
  const contextUsed =
    listed?.tile.context_tokens !== undefined && listed.tile.context_window ? listed.tile.context_tokens / listed.tile.context_window : undefined;

  return (
    <div className="app">
      <nav className="rail" aria-label="Rail">
        <div className="rail-lists">
          <ErrorBoundary what="The projects" resetKey={projectsVersion}>
            <ProjectList view={projects} onNewSession={newSession} onSetTrust={setTrust} trusting={trusting} notes={trustNotes}>
              {view.phase === "connected" && <AddProject onAdd={addProject} adding={adding} />}
            </ProjectList>
          </ErrorBoundary>
          <ErrorBoundary what="The sessions" resetKey={sessionsVersion}>
            <SessionList view={sessions} activeId={composing ? undefined : activeId} onSelect={select} workspaces={workspaceNames} />
          </ErrorBoundary>
        </div>
        <div className="rail-core">
          {quitAsk && (
            <QuitQuestion busy={quitAsk.busy} quitting={quitAsk.quitting} failed={quitAsk.failed} onQuit={quit} onCancel={cancel} />
          )}
          <ErrorBoundary what="Core's status" resetKey={coreVersion} action={<Button onClick={retry}>Retry</Button>}>
            <CoreStatus view={view} onRetry={retry} />
          </ErrorBoundary>
        </div>
      </nav>
      {shown ? (
        <main className="stage">
          {/* Tries again on each new event, and keeps the composer out of it. */}
          <ErrorBoundary what="This session" resetKey={`${shown.id}:${shown.transcript.lastSeq}`}>
            <Transcript
              key={shown.id}
              title={(activeId && titles.current[activeId]) || "Untitled session"}
              untitled={!(activeId && titles.current[activeId])}
              view={shown}
              onDecide={decide}
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
              unavailable={view.phase === "connected" ? undefined : "Core is not connected."}
              sending={shown.message}
              onSend={(text) => follower.current?.send(text) ?? Promise.resolve(false)}
            />
            {listed && sessionValues && listedProject && (
              <SessionControls
                hosts={view.phase === "connected" ? [view.hello.host] : []}
                projects={[listedProject]}
                backends={backends[listed.host_id]}
                values={sessionValues}
                onChoose={() => {}}
                disabled
                used={contextUsed}
              />
            )}
          </ErrorBoundary>
        </main>
      ) : hello && screenProjects && (composing || (view.phase === "connected" && activeId === undefined)) ? (
        <main className="stage">
          <ErrorBoundary what="The new-session screen" resetKey={composing?.projectId ?? ""}>
            <NewSessionScreen
              hosts={[hello.host]}
              projects={screenProjects}
              backends={backends}
              lastUsed={lastUsed}
              projectId={composing?.projectId}
              onStart={start}
              starting={starting}
              onAddProject={addProject}
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
