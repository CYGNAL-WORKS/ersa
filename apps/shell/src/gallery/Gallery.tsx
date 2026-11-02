// The development-only gallery (ADR 0008): every component in every state,
// light and dark side by side. The screenshot tests in e2e/ photograph each
// component section in each theme, so a visual change shows up in review.
// A component built for the window gets an entry here in the same change.

import type { ReactNode } from "react";
import { AddProject } from "../components/AddProject";
import { Button } from "../components/Button";
import { Composer } from "../components/Composer";
import { CoreStatus } from "../components/CoreStatus";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { ProjectList } from "../components/ProjectList";
import { QuitQuestion } from "../components/QuitQuestion";
import { SessionList } from "../components/SessionList";
import { Transcript } from "../components/Transcript";
import type { CoreView } from "../core/connection";
import type { Envelope, Event } from "../protocol/agent-events.gen";
import type { CoreHelloResult, Project, Session } from "../protocol/core-api.gen";
import type { SessionView } from "../session/follow";
import { apply, emptyTranscript } from "../session/transcript";
import "./Gallery.css";

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "WORKSTATION", platform: "windows" },
  capabilities: [],
};

const projects: Project[] = [
  { id: "p1", host_id: "local", path: "D:\\Projects\\ersa", name: "ersa", vcs: "git", trusted: true },
  { id: "p2", host_id: "local", path: "D:\\Projects\\game\\engine\\source\\runtime", name: "engine", vcs: "perforce", trusted: false },
  { id: "p3", host_id: "local", path: "C:\\Users\\me\\notes", name: "notes with a rather long project name", vcs: "none", trusted: false },
  { id: "p4", host_id: "local", path: "D:\\Clones\\stranger", name: "stranger", vcs: "git", trusted: false },
];

const coreViews: [string, CoreView][] = [
  ["Connecting", { phase: "connecting" }],
  [
    "Started",
    {
      phase: "connected",
      link: 1,
      how: "launched",
      hello,
      status: { sessions_running: 3, sessions_waiting: 1, uptime_s: 42, db_path: "" },
      projects: { state: "loading" },
      sessions: { state: "loading" },
    },
  ],
  [
    "Attached, before status",
    { phase: "connected", link: 1, how: "attached", hello, projects: { state: "loading" }, sessions: { state: "loading" } },
  ],
  [
    "Attached to a label",
    {
      phase: "connected",
      link: 1,
      how: "attached",
      label: "replay",
      hello,
      projects: { state: "loading" },
      sessions: { state: "loading" },
    },
  ],
  ["Lost, reconnecting", { phase: "lost", reason: "Core closed the connection", hello }],
  [
    "Unavailable",
    {
      phase: "failed",
      message:
        "Core exited before it started serving (exit code: 1):\nersa-core: cannot open the database at C:\\Users\\me\\AppData\\Local\\Ersa\\ersa.db",
    },
  ],
];

const session = (id: string, title: string, created_at: string, tile: Session["tile"]): Session => ({
  id,
  workspace_id: "w1",
  host_id: "local",
  backend: "claude-code",
  title,
  state: tile.state,
  created_at,
  tile,
});

const sessions: Session[] = [
  session("s1", "Fix the flaky reconnect test", "2026-11-01T09:00:00Z", {
    state: "waiting_user",
    pending: [{ kind: "permission", request_id: "r1" }],
    cost_usd_total: 0.82,
  }),
  session("s2", "Tidy the gallery", "2026-11-01T08:00:00Z", { state: "running", pending: [], cost_usd_total: 0.19 }),
  session("s3", "Explain the raw log", "2026-11-01T07:00:00Z", { state: "idle", pending: [], cost_usd_total: 12.4 }),
  session("s4", "Start-up", "2026-11-01T10:00:00Z", { state: "starting", pending: [] }),
  session("s5", "A session whose backend exited with an error", "2026-11-01T06:00:00Z", { state: "error", pending: [] }),
];

/** A transcript built the way the window builds one: by applying events. */
function transcriptOf(events: Event[]) {
  return events
    .map(
      (event, seq): Envelope => ({
        v: 2,
        session_id: "s1",
        seq,
        ts: "2026-11-01T09:00:00Z",
        source: { backend: "claude-code", backend_version: "2.1.302", input: "live" },
        event,
      }),
    )
    .reduce(apply, emptyTranscript);
}

const command = { command: "echo probe > probe.txt", description: "Write the word probe to probe.txt" };
const flags = { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false };
const asked: Event[] = [
  { type: "session.state", state: "running" },
  {
    type: "user.message",
    origin: "human",
    text: "Use the Bash tool to run exactly this command: echo probe > probe.txt. Then report whether it succeeded.",
  },
  { type: "assistant.text", block_id: "m1/0", text: "Running the requested command now.", final: true },
  { type: "tool.requested", tool_use_id: "t1", name: "Bash", input: command },
  {
    type: "permission.requested",
    request_id: "r1",
    tool_use_id: "t1",
    tool_name: "Bash",
    description: "Write the word probe to probe.txt",
    input: command,
    blocked_path: "C:\\Users\\me\\AppData\\Local\\Temp\\probe.txt",
    suggestions: [],
    flags,
  },
  { type: "session.state", state: "waiting_user" },
];
// An agent hiding what a command does: a right-to-left override and a zero-width space, shown as code points.
const hiding: Event[] = [
  {
    type: "permission.requested",
    request_id: "r2",
    tool_use_id: "t2",
    tool_name: "Bash",
    input: { command: "echo safe \u202E; rm -rf ~ #\u200B" },
    suggestions: [],
    flags,
  },
];
const answered: Event[] = [
  ...asked,
  { type: "permission.decided", request_id: "r1", behavior: "allow", decided_by: "user" },
  { type: "tool.result", tool_use_id: "t1", content: "(Bash completed with no output)", is_error: false },
  {
    type: "assistant.text",
    block_id: "m2/0",
    text: "The command succeeded. It exited cleanly with no output, and probe.txt now exists.",
    final: true,
  },
  { type: "session.state", state: "idle" },
];
const streaming: Event[] = [
  { type: "session.state", state: "running" },
  { type: "user.message", origin: "human", text: "Say ready." },
  { type: "tool.requested", tool_use_id: "t0", name: "Read", input: { file_path: "docs/design/DESIGN.md" } },
  { type: "tool.progress", tool_use_id: "t0", detail: "Reading DESIGN.md" },
  { type: "assistant.text", block_id: "m1/0", text: "Ready, and reading the", final: false },
];
const failing: Event[] = [
  { type: "user.message", origin: "human", text: "Run the tests." },
  { type: "tool.requested", tool_use_id: "t1", name: "Bash", input: { command: "cargo test" } },
  { type: "tool.result", tool_use_id: "t1", content: "error: could not compile `ersa-core`", is_error: true },
  { type: "error", code: "backend_exited", message: "Claude Code exited unexpectedly (exit code 1).", recoverable: false },
  { type: "session.ended", reason: "error", exit_code: 1 },
  { type: "session.state", state: "ended" },
];

const sessionView = (events: Event[], more: Partial<SessionView> = {}): SessionView => ({
  id: "s1",
  transcript: transcriptOf(events),
  stream: { state: "live" },
  answers: {},
  ...more,
});

// What the user sees of Core's refusal: its message without the method name (describeError).
const refusal = "the session is a replay of a recording and cannot change";

const transcriptStates: [string, SessionView][] = [
  ["Loading", sessionView([], { stream: { state: "subscribing" } })],
  ["Empty", sessionView([])],
  ["Streaming", sessionView(streaming)],
  ["Permission open", sessionView(asked)],
  ["Permission sending", sessionView(asked, { answers: { r1: { state: "sending" } } })],
  ["Permission refused", sessionView(asked, { answers: { r1: { state: "failed", message: refusal } } })],
  ["Permission while Core is away", sessionView(asked, { stream: { state: "offline" } })],
  ["Hidden characters", sessionView(hiding)],
  ["Answered", sessionView(answered)],
  ["Failed", sessionView(failing)],
  ["Not receiving events", sessionView([], { stream: { state: "failed", message: "Core closed the connection" } })],
];

/** Throws while rendering, as a component given data it cannot read would. */
function Unreadable(): never {
  throw new Error("Cannot read properties of undefined (reading 'toFixed')");
}

interface Entry {
  name: string;
  states: [string, ReactNode][];
  /** Shown one state per row, at the stage's width, rather than in rail-width columns. */
  wide?: boolean;
}

const entries: Entry[] = [
  {
    name: "CoreStatus",
    states: coreViews.map(([label, view]) => [label, <CoreStatus view={view} onRetry={() => {}} />]),
  },
  {
    name: "QuitQuestion",
    states: [
      ["One session busy", <QuitQuestion busy={1} onQuit={() => {}} onCancel={() => {}} takeFocus={false} />],
      ["Sessions not counted", <QuitQuestion busy={undefined} onQuit={() => {}} onCancel={() => {}} takeFocus={false} />],
      [
        "Several, as it opens",
        <QuitQuestion busy={3} onQuit={() => {}} onCancel={() => {}} takeFocus={false} forceCancelFocus />,
      ],
      ["Quitting", <QuitQuestion busy={3} onQuit={() => {}} onCancel={() => {}} takeFocus={false} quitting />],
      [
        "Quit failed",
        <QuitQuestion busy={2} onQuit={() => {}} onCancel={() => {}} takeFocus={false} failed="Ersa did not quit: not attached to Core" />,
      ],
    ],
  },
  {
    name: "ProjectList",
    states: [
      ["Loading", <ProjectList view={{ state: "loading" }} />],
      ["Empty", <ProjectList view={{ state: "ready", projects: [] }} />],
      ["Projects", <ProjectList view={{ state: "ready", projects }} />],
      ["Error", <ProjectList view={{ state: "error", message: "Core could not read its database." }} />],
      ["Core unavailable", <ProjectList view={{ state: "unavailable" }} />],
      [
        "Starting a session",
        <ProjectList view={{ state: "ready", projects }} onNewSession={() => {}} starting={{ p1: { state: "sending" } }} />,
      ],
      ["Trust controls", <ProjectList view={{ state: "ready", projects }} onNewSession={() => {}} onSetTrust={() => {}} />],
      [
        "Asking to trust",
        <ProjectList view={{ state: "ready", projects }} onNewSession={() => {}} onSetTrust={() => {}} initialAsking="p4" />,
      ],
      [
        "Changing trust",
        <ProjectList
          view={{ state: "ready", projects }}
          onNewSession={() => {}}
          onSetTrust={() => {}}
          trusting={{ p1: { state: "sending" }, p4: { state: "sending" } }}
        />,
      ],
      [
        "Trust withdrawn, sessions running",
        <ProjectList
          view={{ state: "ready", projects: projects.map((p) => (p.id === "p1" ? { ...p, trusted: false } : p)) }}
          onNewSession={() => {}}
          onSetTrust={() => {}}
          notes={{ p1: "Running sessions keep their current settings until they are stopped." }}
        />,
      ],
      [
        "Trust refused",
        <ProjectList
          view={{ state: "ready", projects }}
          onNewSession={() => {}}
          onSetTrust={() => {}}
          trusting={{ p4: { state: "failed", message: "no project p4" } }}
        />,
      ],
      [
        "Session refused",
        <ProjectList
          view={{ state: "ready", projects }}
          onNewSession={() => {}}
          starting={{ p1: { state: "failed", message: "this Core is a replay server and changes nothing" } }}
        />,
      ],
    ],
  },
  {
    name: "AddProject",
    states: [
      ["Empty", <AddProject onAdd={async () => false} />],
      ["Typed", <AddProject onAdd={async () => false} initialPath="D:\Projects\ersa" />],
      ["Hover", <AddProject onAdd={async () => false} force="hover" />],
      ["Focus", <AddProject onAdd={async () => false} initialPath="D:\Projects\ersa" force="focus" />],
      ["Asking to trust", <AddProject onAdd={async () => false} initialPath="D:\Clones\stranger" initialAsking />],
      [
        "Asking, from the keyboard",
        <AddProject onAdd={async () => false} initialPath="D:\Clones\stranger" initialAsking forceCancelFocus />,
      ],
      ["Adding", <AddProject onAdd={async () => false} initialPath="D:\Projects\ersa" adding={{ state: "sending" }} />],
      [
        "Refused",
        <AddProject
          onAdd={async () => false}
          initialPath="D:\Projects\nowhere"
          adding={{ state: "failed", message: "D:\\Projects\\nowhere does not exist" }}
        />,
      ],
    ],
  },
  {
    name: "SessionList",
    states: [
      ["Loading", <SessionList view={{ state: "loading" }} onSelect={() => {}} />],
      ["Empty", <SessionList view={{ state: "ready", sessions: [] }} onSelect={() => {}} />],
      ["Sessions", <SessionList view={{ state: "ready", sessions }} activeId="s2" onSelect={() => {}} />],
      [
        "Hover",
        <SessionList view={{ state: "ready", sessions }} activeId="s2" onSelect={() => {}} force={{ id: "s3", state: "hover" }} />,
      ],
      [
        "Focus on the active tile",
        <SessionList view={{ state: "ready", sessions }} activeId="s2" onSelect={() => {}} force={{ id: "s2", state: "focus" }} />,
      ],
      [
        "Pressed",
        <SessionList view={{ state: "ready", sessions }} activeId="s2" onSelect={() => {}} force={{ id: "s3", state: "pressed" }} />,
      ],
      ["Error", <SessionList view={{ state: "error", message: "Core could not read its database." }} onSelect={() => {}} />],
      ["Core unavailable", <SessionList view={{ state: "unavailable" }} onSelect={() => {}} />],
    ],
  },
  {
    name: "Transcript",
    wide: true,
    states: transcriptStates.map(([label, view]) => [
      label,
      <Transcript title="Fix the flaky reconnect test" view={view} onDecide={() => {}} onRetry={() => {}} />,
    ]),
  },
  {
    name: "ErrorBoundary",
    wide: true,
    states: [
      [
        "A session that cannot be shown",
        <ErrorBoundary what="This session">
          <Unreadable />
        </ErrorBoundary>,
      ],
    ],
  },
  {
    name: "Composer",
    wide: true,
    states: [
      ["Empty", <Composer autoFocus={false} onSend={async () => false} />],
      ["Hover", <Composer autoFocus={false} onSend={async () => false} force="hover" />],
      ["Focus", <Composer autoFocus={false} onSend={async () => false} force="focus" />],
      ["Typed", <Composer autoFocus={false} onSend={async () => false} initialText="Run the tests again, then summarise what failed." />],
      ["Sending", <Composer autoFocus={false} onSend={async () => false} initialText="Run the tests again." sending={{ state: "sending" }} />],
      [
        "Refused",
        <Composer
          autoFocus={false}
          onSend={async () => false}
          initialText="Run the tests again."
          sending={{ state: "failed", message: refusal }}
        />,
      ],
      ["Core not connected", <Composer autoFocus={false} onSend={async () => false} unavailable="Core is not connected." />],
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
      ["Primary", <Button variant="primary">Allow</Button>],
      ["Primary hover", <Button variant="primary" force="hover">Allow</Button>],
      ["Primary focus", <Button variant="primary" force="focus">Allow</Button>],
      ["Primary pressed", <Button variant="primary" force="pressed">Allow</Button>],
      ["Primary disabled", <Button variant="primary" disabled>Allow</Button>],
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
              <div className="gallery-heading">
                <h2 className="gallery-name">{entry.name}</h2>
                <span className="gallery-theme-name">{theme}</span>
              </div>
              <div className={entry.wide ? "gallery-states gallery-states-wide" : "gallery-states"}>
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
