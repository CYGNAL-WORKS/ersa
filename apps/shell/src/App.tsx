import { useCallback, useEffect, useRef, useState } from "react";
import "./App.css";
import { AddProject } from "./components/AddProject";
import { Composer } from "./components/Composer";
import { CoreStatus } from "./components/CoreStatus";
import { ProjectList } from "./components/ProjectList";
import { SessionList, newestFirst } from "./components/SessionList";
import { Transcript } from "./components/Transcript";
import { type CoreClient, describeError } from "./core/client";
import { type Connection, type CoreView, type ProjectsView, type SessionsView, connect } from "./core/connection";
import type { Project } from "./protocol/core-api.gen";
import { type Follower, type SessionView, type Sending, follow } from "./session/follow";

/** The backend a new session runs. The only one until M3 (FLOW-NEW-SESSION leaves the choice open). */
const BACKEND = "claude-code";

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

function sessionsOf(view: CoreView): SessionsView {
  switch (view.phase) {
    case "connected":
      return view.sessions;
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
  const connection = useRef<Connection | null>(null);

  // The session the stage shows, and what the window knows of it.
  const [activeId, setActiveId] = useState<string>();
  const [session, setSession] = useState<SessionView>();
  const follower = useRef<Follower | null>(null);

  const [starting, setStarting] = useState<Record<string, Sending>>({});
  const [adding, setAdding] = useState<Sending>();

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

  // Subscribe on each new connection, from where the transcript left off.
  const link = view.phase === "connected" ? view.link : undefined;
  useEffect(() => {
    if (link !== undefined) follower.current?.attached();
  }, [link, activeId]);

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

  const addProject = async (path: string) => {
    if (view.phase !== "connected") return false;
    setAdding({ state: "sending" });
    try {
      await client.call("project.add", { host_id: view.hello.host.id, path });
      setAdding(undefined);
      connection.current?.reload("projects");
      return true;
    } catch (error) {
      setAdding({ state: "failed", message: describeError(error) });
      return false;
    }
  };

  // The title as last listed, so the header keeps it while the list reloads
  // or Core is away.
  const titles = useRef<Record<string, string>>({});
  if (sessions.state === "ready") for (const s of sessions.sessions) if (s.title) titles.current[s.id] = s.title;
  const shown = session && session.id === activeId ? session : undefined;

  return (
    <div className="app">
      <nav className="rail" aria-label="Rail">
        <div className="rail-lists">
          <ProjectList view={projectsOf(view)} onNewSession={newSession} starting={starting}>
            {view.phase === "connected" && <AddProject onAdd={addProject} adding={adding} />}
          </ProjectList>
          <SessionList view={sessions} activeId={activeId} onSelect={setActiveId} />
        </div>
        <CoreStatus view={view} onRetry={retry} />
      </nav>
      {shown ? (
        <main className="stage">
          <Transcript
            key={shown.id}
            title={(activeId && titles.current[activeId]) || "Untitled session"}
            view={shown}
            onDecide={(requestId, behavior) => follower.current?.decide(requestId, behavior)}
          />
          <Composer
            key={shown.id}
            unavailable={view.phase === "connected" ? undefined : "Core is not connected."}
            sending={shown.message}
            onSend={(text) => follower.current?.send(text) ?? Promise.resolve(false)}
          />
        </main>
      ) : (
        <main className="stage stage-empty">No session open. Start one from a project in the rail.</main>
      )}
    </div>
  );
}
