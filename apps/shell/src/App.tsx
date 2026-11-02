import { useCallback, useEffect, useRef, useState } from "react";
import "./App.css";
import { AddProject } from "./components/AddProject";
import { Button } from "./components/Button";
import { Composer } from "./components/Composer";
import { CoreStatus } from "./components/CoreStatus";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ProjectList } from "./components/ProjectList";
import { SessionList, newestFirst } from "./components/SessionList";
import { Transcript } from "./components/Transcript";
import { type CoreClient, describeError } from "./core/client";
import { type Connection, type CoreView, type ProjectsView, type SessionsView, connect } from "./core/connection";
import type { Project } from "./protocol/core-api.gen";
import { type Follower, type SessionView, type Sending, follow } from "./session/follow";

/** The backend a new session runs. The only one until M3 (FLOW-NEW-SESSION leaves the choice open). */
const BACKEND = "claude-code";

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

export function App({ client }: { client: CoreClient }) {
  const [view, setView] = useState<CoreView>({ phase: "connecting" });
  // Bumped to attach again; the effect below reconnects on each change.
  const [attempt, setAttempt] = useState(0);
  const retry = useCallback(() => setAttempt((n) => n + 1), []);
  const connection = useRef<Connection | null>(null);

  // The session the stage shows, and what the window knows of it.
  const [activeId, setActiveId] = useState<string>();
  const [session, setSession] = useState<SessionView>();
  const follower = useRef<Follower | null>(null);

  const [starting, setStarting] = useState<Record<string, Sending>>({});
  const [trusting, setTrusting] = useState<Record<string, Sending>>({});
  // The workspaces of each project whose trust the user withdrew here, as they were then. A session running
  // in one of them keeps the configuration it started with.
  const [withdrawn, setWithdrawn] = useState<Record<string, string[]>>({});
  const [adding, setAdding] = useState<Sending>();
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

  // Each list's error boundary tries again whenever the list changes.
  const projects = projectsOf(view);
  const projectsVersion = useVersion(projects);
  const sessionsVersion = useVersion(sessions);
  const coreVersion = useVersion(view);
  const retrySubscribe = useCallback(() => follower.current?.attached(), []);

  // Stable, so the transcript's memoised blocks do not re-render for it.
  const decide = useCallback((requestId: string, behavior: "allow" | "deny") => follower.current?.decide(requestId, behavior), []);

  const newSession = async (project: Project) => {
    setStarting((s) => ({ ...s, [project.id]: { state: "sending" } }));
    try {
      const workspace = await client.call("workspace.create", { project_id: project.id });
      const created = await client.call("session.create", { workspace_id: workspace.id, backend: BACKEND });
      setStarting(({ [project.id]: _, ...rest }) => rest);
      connection.current?.reload("sessions");
      setActiveId(created.id);
    } catch (error) {
      setStarting((s) => ({ ...s, [project.id]: { state: "failed", message: describeError(error) } }));
    }
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
  const shown = session && session.id === activeId ? session : undefined;

  return (
    <div className="app">
      <nav className="rail" aria-label="Rail">
        <div className="rail-lists">
          <ErrorBoundary what="The projects" resetKey={projectsVersion}>
            <ProjectList view={projects} onNewSession={newSession} starting={starting} onSetTrust={setTrust} trusting={trusting} notes={trustNotes}>
              {view.phase === "connected" && <AddProject onAdd={addProject} adding={adding} />}
            </ProjectList>
          </ErrorBoundary>
          <ErrorBoundary what="The sessions" resetKey={sessionsVersion}>
            <SessionList view={sessions} activeId={activeId} onSelect={setActiveId} />
          </ErrorBoundary>
        </div>
        <ErrorBoundary what="Core's status" resetKey={coreVersion} action={<Button onClick={retry}>Retry</Button>}>
          <CoreStatus view={view} onRetry={retry} />
        </ErrorBoundary>
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
          </ErrorBoundary>
        </main>
      ) : (
        <main className="stage stage-empty">No session open. Start one from a project in the rail.</main>
      )}
    </div>
  );
}
