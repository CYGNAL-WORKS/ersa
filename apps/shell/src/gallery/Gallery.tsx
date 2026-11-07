// The development-only gallery (ADR 0008): every component in every state,
// light and dark side by side. The screenshot tests in e2e/ photograph each
// component section in each theme, so a visual change shows up in review.
// A component built for the window gets an entry here in the same change.

import { ChevronDown, ChevronRight, Plus, ShieldAlert, X } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { Button } from "../components/Button";
import { Chip } from "../components/Chip";
import { ContextRing } from "../components/ContextRing";
import { Composer } from "../components/Composer";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { Icon, IconButton } from "../components/Icon";
import { NewSessionScreen } from "../components/NewSessionScreen";
import { QuitQuestion } from "../components/QuitQuestion";
import { RailFoot } from "../components/RailFoot";
import { RailTop } from "../components/RailTop";
import { RailTree } from "../components/RailTree";
import { SessionControls } from "../components/SessionControls";
import { Shelf } from "../components/Shelf";
import { SessionTitle } from "../components/SessionTitle";
import { Transcript } from "../components/Transcript";
import type { CoreView } from "../core/connection";
import type { Envelope, Event } from "../protocol/agent-events.gen";
import type { CoreHelloResult, Project, Session } from "../protocol/core-api.gen";
import type { Answer } from "../session/answers";
import { type BackendChoices, type Choice, branchesMatching } from "../session/controls";
import type { SessionView } from "../session/follow";
import { type ShelfShows, shelfTone } from "../session/shelf";
import { tileOf } from "../session/tile";
import { EVENT_VERSION, apply, emptyTranscript } from "../session/transcript";
import "./Gallery.css";

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "WORKSTATION", platform: "windows" },
  capabilities: [],
};

const projects: Project[] = [
  { id: "p1", host_id: "local", path: "C:\\Users\\me\\ersa", name: "ersa", vcs: "git", trusted: true },
  { id: "p2", host_id: "local", path: "D:\\Projects\\game\\engine\\source\\runtime", name: "engine", vcs: "perforce", trusted: false },
  { id: "p3", host_id: "local", path: "C:\\Users\\me\\notes", name: "notes with a rather long project name", vcs: "none", trusted: false },
  { id: "p4", host_id: "local", path: "D:\\Clones\\stranger", name: "stranger", vcs: "git", trusted: false },
];

const connected: CoreView = {
  phase: "connected",
  link: 1,
  how: "launched",
  hello,
  status: { sessions_running: 3, sessions_waiting: 1, uptime_s: 42, db_path: "" },
  projects: { state: "loading" },
  sessions: { state: "loading" },
};

const coreViews: [string, CoreView][] = [
  // The first two draw nothing: the foot is silent while all is well.
  ["Connecting", { phase: "connecting" }],
  ["Connected", connected],
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
  ["Reconnecting", { phase: "lost", reason: "The connection closed.", hello }],
  ["Reconnecting, no reason", { phase: "lost", reason: "", hello }],
  [
    "Unreachable",
    {
      phase: "failed",
      message:
        "Ersa's background process stopped before it was ready. It ended (exit code: 1).\nersa-core: cannot open the database at C:\\Users\\me\\AppData\\Local\\Ersa\\ersa.db",
    },
  ],
];

/**
 * The foot as Retry drives it, for the browser tests: Ersa connects for a moment, then the first Retry fails again
 * and the next one reaches the sessions. As in the window, Retry only counts an attempt, and connecting starts in an
 * effect after the foot has drawn once more with Retry still there.
 */
function RetryingFoot({ view }: { view: CoreView }) {
  const [shown, setShown] = useState(view);
  const [tries, setTries] = useState(0);
  useEffect(() => {
    if (tries === 0) return;
    setShown({ phase: "connecting" });
    const timer = setTimeout(() => setShown(tries === 1 ? view : connected), 500);
    return () => clearTimeout(timer);
  }, [tries, view]);
  return <RailFoot view={shown} onRetry={() => setTries((n) => n + 1)} />;
}

const session = (id: string, title: string, created_at: string, tile: Session["tile"]): Session => ({
  id,
  workspace_id: "w1",
  host_id: "local",
  backend: "claude-code",
  model: "opus",
  effort: "medium",
  permission_mode: "default",
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

/** A tile in every state and want RAIL.md gives, newest first in this order. */
const ring = { context_tokens: 84_000, context_window: 200_000 };
const everyState: Session[] = (
  [
    ["Start-up", { state: "starting", pending: [] }],
    ["Tidy the gallery", { state: "running", pending: [], cost_usd_total: 0.19, ...ring }],
    ["Fix the flaky reconnect test", { state: "waiting_user", pending: [{ kind: "permission", request_id: "a" }], cost_usd_total: 0.82, ...ring }],
    ["Rotate the runner image", { state: "waiting_user", pending: [{ kind: "question", request_id: "b" }], ...ring }],
    [
      "Port the replay server",
      {
        state: "waiting_user",
        pending: [
          { kind: "question", request_id: "c" },
          { kind: "permission", request_id: "d" },
          { kind: "question", request_id: "e" },
        ],
        cost_usd_total: 3.1,
        ...ring,
      },
    ],
    ["Three questions", { state: "running", pending: ["f", "g", "h"].map((id) => ({ kind: "question", request_id: id })) }],
    ["A request of a newer kind", { state: "waiting_user", pending: [{ kind: "approval", request_id: "i" } as unknown as Session["tile"]["pending"][number]] }],
    ["Explain the raw log", { state: "idle", pending: [], cost_usd_total: 12.4, ...ring }],
    ["Summarize the protocol briefs", { state: "idle", pending: [], cost_usd_total: 8.12, context_tokens: 182_000, context_window: 200_000 }],
    ["Waiting, with nothing pending", { state: "waiting_user", pending: [] }],
    ["Sparse WSL disks", { state: "disconnected", pending: [], cost_usd_total: 0.44, ...ring }],
    ["Rename across hosts", { state: "error", pending: [], cost_usd_total: 0.02, ...ring }],
    ["Evaluate the DeepSeek harness", { state: "ended", pending: [], cost_usd_total: 3.9, ...ring }],
    ["A state from a newer Core", { state: "paused" as unknown as Session["tile"]["state"], pending: [] }],
    ["", { state: "idle", pending: [] }],
  ] as const satisfies readonly (readonly [string, Session["tile"]])[]
).map(([title, tile], i) => ({
  ...session(`e${i}`, title, `2026-11-01T${String(20 - i).padStart(2, "0")}:00:00Z`, tile),
  title: title || undefined,
}));

/** Two tiles, the second of which shows a pointer, keyboard or active state. */
const pointerPair: Session[] = [
  session("p1", "At rest", "2026-11-01T10:00:00Z", { state: "idle", pending: [], cost_usd_total: 0.82, ...ring }),
  session("p2", "The one in question", "2026-11-01T09:00:00Z", { state: "running", pending: [], cost_usd_total: 0.82, ...ring }),
];

/** The rail's tree: ersa here with its sessions, a project on another machine with two, and two without any. */
const treeProjects: Project[] = [
  projects[0]!,
  { id: "p5", host_id: "devbox", path: "/home/me/runner", name: "runner", vcs: "git", trusted: true },
  projects[1]!,
  projects[3]!,
];
const treeWorkspaces = { w1: { project_id: "p1" }, "w-dev": { project_id: "p5" } };
const treeSessions = sessions.map((s) => (s.id === "s4" || s.id === "s5" ? { ...s, workspace_id: "w-dev" } : s));
const tree: Parameters<typeof RailTree>[0] = {
  projects: { state: "ready", projects: treeProjects },
  sessions: { state: "ready", sessions: treeSessions },
  workspaces: treeWorkspaces,
  activeId: "s2",
  onSelect: () => {},
  // The last call a heading made, kept on the page where the gallery test reads it; nothing shows it.
  onNewSession: (project) => (document.body.dataset.railCall = `newSession ${project.id}`),
  onSetTrust: (project, trusted) => (document.body.dataset.railCall = `setTrust ${project.id} ${trusted}`),
  localHost: "local",
  hostNames: { local: "WORKSTATION", devbox: "devbox" },
};
/** One project, for the states of a heading and its tiles. */
const ersaOnly: Parameters<typeof RailTree>[0] = {
  ...tree,
  projects: { state: "ready", projects: [projects[0]!] },
  sessions: { state: "ready", sessions: treeSessions.filter((s) => s.workspace_id === "w1") },
};
const strangerOnly: Parameters<typeof RailTree>[0] = { ...tree, projects: { state: "ready", projects: [projects[3]!] }, sessions: { state: "ready", sessions: [] } };
/** A resting tile above each forced one, so the two read apart in the screenshot. */
const pairOnly: Parameters<typeof RailTree>[0] = { ...ersaOnly, sessions: { state: "ready", sessions: pointerPair }, activeId: undefined };
/** ersa's sessions with a question in place of its permission request, for a collapsed heading in the accent. */
const askingOnly: Parameters<typeof RailTree>[0] = {
  ...ersaOnly,
  sessions: {
    state: "ready",
    sessions: treeSessions
      .filter((s) => s.workspace_id === "w1")
      .map((s) => (s.tile.pending.length > 0 ? { ...s, tile: { ...s.tile, pending: [{ kind: "question" as const, request_id: "q1" }] } } : s)),
  },
};
const folded = (...ids: string[]) => new Set(ids);

/** A transcript built the way the window builds one: by applying events. */
function transcriptOf(events: Event[]) {
  return events
    .map(
      (event, seq): Envelope => ({
        v: EVENT_VERSION,
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
// A second turn after the first, so the gap between turns is seen.
const twoTurns: Event[] = [
  ...answered,
  { type: "user.message", origin: "human", text: "Now delete probe.txt." },
  { type: "assistant.text", block_id: "m3/0", text: "Deleted. probe.txt no longer exists.", final: true },
];
// A path with no break in it, which must wrap inside the bubble rather than widen it.
const longPath: Event[] = [
  {
    type: "user.message",
    origin: "human",
    text: "Read C:\\Users\\me\\Projects\\ersa\\target\\debug\\build\\ersa-core-5f3c2a9e1b7d4c08\\out\\generated_protocol_bindings_for_the_session_stream.rs and summarize it.",
  },
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
  ...more,
});

// What the user sees of Core's refusal: its message without the method name (describeError).
const refusal = "the session is a replay of a recording and cannot change";

const transcriptStates: [string, SessionView, answers?: Record<string, Answer>][] = [
  ["Loading", sessionView([], { stream: { state: "subscribing" } })],
  ["Empty", sessionView([])],
  ["Streaming", sessionView(streaming)],
  ["Permission open", sessionView(asked)],
  ["Permission sending", sessionView(asked), { r1: { state: "sending", decision: "allow" } }],
  ["Permission refused", sessionView(asked), { r1: { state: "failed", message: refusal } }],
  ["Permission while Core is away", sessionView(asked, { stream: { state: "offline" } })],
  ["Hidden characters", sessionView(hiding)],
  ["Answered", sessionView(answered)],
  ["Two turns", sessionView(twoTurns)],
  ["Long path", sessionView(longPath)],
  ["Failed", sessionView(failing)],
  ["Not receiving events", sessionView([], { stream: { state: "failed", message: "Core closed the connection" } })],
];

/** The session title as the transcript header holds it, beside the session's state. */
function Titled(props: Omit<Parameters<typeof SessionTitle>[0], "title" | "onRename" | "children">) {
  return (
    <header className="transcript-header">
      <SessionTitle title="Fix the flaky reconnect test" onRename={async () => false} {...props}>
        <span className="transcript-state">Idle</span>
      </SessionTitle>
    </header>
  );
}

const draft = "Fix the flaky reconnect test on Windows";

// What the Claude Code adapter lists for `backend.list` (PROTO-CLAUDE-CODE-STREAM), with the CLI's default marked.
const choices = (pairs: [string, string][], dflt: string): Choice[] =>
  pairs.map(([id, label]) => ({ id, label, default: id === dflt }));
const claudeCode: BackendChoices = {
  name: "claude-code",
  label: "Claude Code",
  version: "2.1.302",
  status: "available",
  tested: { min: "2.1.0", max: "2.1.400" },
  models: choices([["fable", "Fable"], ["opus", "Opus"], ["sonnet", "Sonnet"], ["haiku", "Haiku"]], "opus"),
  efforts: choices([["low", "Low"], ["medium", "Medium"], ["high", "High"], ["xhigh", "Extra high"], ["max", "Max"]], "high"),
  permission_modes: choices(
    [["default", "Ask before acting"], ["acceptEdits", "Accept edits"], ["plan", "Plan only"], ["auto", "Auto"]],
    "default",
  ),
};
const copilot = (status: BackendChoices["status"]): BackendChoices => ({
  name: "copilot",
  label: "GitHub Copilot",
  status,
  tested: { min: "1.0.0", max: "1.2.0" },
  models: [],
  efforts: [],
  permission_modes: [],
});

const galleryBranches = [
  { name: "feature/rail-tree", default: false },
  { name: "feature/workspace-chip", default: false },
  { name: "fix/reconnect", default: false },
  { name: "main", default: true },
  { name: "release/0.1", default: false },
];
const branchOptions = galleryBranches.map((b) => ({ id: b.name, label: b.name, status: b.default ? "Default" : undefined }));
/** The workspace chip's base branch section, saying `empty` with nothing to list (docs/design/COMPOSER.md, The workspace list). */
const baseSection = (listed: typeof branchOptions, empty = "No branch matches.") => ({
  heading: "Base branch",
  placeholder: "Search branches",
  narrow: (query: string) => {
    const names = new Set(branchesMatching(listed.map((o) => ({ name: o.id, default: false })), query).map((b) => b.name));
    return listed.filter((o) => names.has(o.id));
  },
  value: "main",
  onChoose: () => {},
  empty,
});

const newSession = {
  hosts: [hello.host],
  localHostId: hello.host.id,
  projects,
  backends: { local: [claudeCode, copilot("missing")] },
  onStart: async () => false,
  // The folder dialog is there, so Add project… can be taken; in the gallery it is always canceled.
  onChooseFolder: async () => ({ kind: "canceled" }) as const,
  onAddProject: async () => undefined,
  // A git project's branches, for a new workspace's base branch.
  onListBranches: async () => ({ state: "read", branches: galleryBranches }) as const,
  autoFocus: false,
} as const;
const oneBackend = { local: [claudeCode] };

/** Room beneath a specimen for a chip's open choices, which lie over what follows. */
const room = (node: ReactNode) => <div className="gallery-room">{node}</div>;
/** Room above a composer for its status line, which grows upward over what is above it. */
const statusRoom = (node: ReactNode) => <div className="gallery-status-room">{node}</div>;
/** More room, for a list with a section beneath its options. */
const tallRoom = (node: ReactNode) => <div className="gallery-room gallery-room-tall">{node}</div>;

const chipOptions = [
  { id: "isolated", label: "New workspace", status: "Isolated" },
  { id: "root", label: "Project folder", status: "Shared" },
];
const chip = { name: "Workspace", value: "isolated", options: chipOptions, onChoose: () => {} };
const projectChip = {
  name: "Project",
  value: "p1",
  wide: true,
  options: [
    { id: "p1", label: "ersa", detail: projects[0]!.path, status: "Git" },
    { id: "p2", label: "engine", detail: `${projects[1]!.path}\\Plugins\\Experimental\\Deep`, status: "Folder", caution: "Not trusted" },
  ],
  action: { label: "Add project…", icon: Plus, shortcut: "Ctrl+O" },
  onChoose: () => {},
  initialOpen: true,
};

/** Three tiles in a rail with the middle one's shelf open beside it, over a strip of the stage, as the window shows it. */
const shelfTiles: Session[] = [
  session("h1", "Tidy the gallery", "2026-11-01T10:00:00Z", { state: "running", pending: [], cost_usd_total: 0.19, ...ring }),
  session("h2", "Fix the flaky reconnect test", "2026-11-01T09:00:00Z", {
    state: "waiting_user",
    pending: [{ kind: "permission", request_id: "r1" }],
    cost_usd_total: 0.82,
    context_tokens: 106_000,
    context_window: 200_000,
  }),
  session("h3", "Explain the raw log", "2026-11-01T08:00:00Z", { state: "idle", pending: [], cost_usd_total: 12.4, ...ring }),
];

interface ShelfMore {
  short?: boolean;
  focus?: boolean;
  answer?: Answer;
  live?: boolean;
}

function ShelfOpen({ shows, more = {} }: { shows: ShelfShows; more?: ShelfMore }) {
  const { short, answer, live = true, focus } = more;
  // The open tile waits on what its shelf shows: a question's tile says it has one.
  const listed = shelfTiles[1] as Session;
  const open: Session =
    shows.state === "question" ? { ...listed, tile: { ...listed.tile, pending: [{ kind: "question", request_id: shows.requestId }] } } : listed;
  const tiles = [shelfTiles[0] as Session, open, shelfTiles[2] as Session];
  const tile = tileOf(open);
  return (
    <div className="gallery-shelf" data-short={short || undefined}>
      <div className="gallery-shelf-rail">
        <RailTree
          projects={{ state: "ready", projects: [projects[0]!] }}
          sessions={{ state: "ready", sessions: tiles }}
          workspaces={{ w1: { project_id: "p1" } }}
          activeId="h1"
          onSelect={() => {}}
          shelf={{
            id: open.id,
            tone: shelfTone(shows),
            render: (domId) => (
              <Shelf id={domId} tile={tile} shows={shows} onRetry={() => {}} answers={answer && { [shelfRequest.request_id]: answer }} live={live} onDecide={() => {}} force={focus ? "focus" : undefined} />
            ),
          }}
        />
      </div>
    </div>
  );
}

const shelfRequest = asked.find((e) => e.type === "permission.requested") as Extract<Event, { type: "permission.requested" }>;
const longCommand = Array.from({ length: 14 }, (_, i) => `cargo test -p ersa-core --test reconnect_${i + 1} -- --nocapture`).join(" &&\n");
const shelfStates: [string, ShelfShows, more?: ShelfMore][] = [
  ["A permission request", { state: "permission", request: shelfRequest }],
  [
    "With a suggestion, one of three",
    {
      state: "permission",
      request: {
        ...shelfRequest,
        suggestions: [{ kind: "allow_rule", label: "Allow echo commands in this project for the rest of the session", payload: {} }],
        reason: { kind: "rule", text: "Writing outside the workspace needs approval." },
      },
      place: { n: 1, m: 3 },
    },
  ],
  ["The last of three, still counted", { state: "permission", request: { ...shelfRequest, description: "Run the reconnect tests again" }, place: { n: 3, m: 3 } }],
  ["Defaulting to no", { state: "permission", request: { ...shelfRequest, blocked_path: undefined, flags: { ...flags, default_to_no: true } } }],
  ["A long input, which scrolls", { state: "permission", request: { ...shelfRequest, description: "Run every reconnect test", input: { command: longCommand } } }],
  ["A question", { state: "question", requestId: "q1", questions: [{ id: "a", text: "Should the reconnect test retry on Windows only, or everywhere?", multi_select: false }] }],
  ["Loading", { state: "loading" }],
  ["Reading failed", { state: "failed", message: "the session's events could not be read" }],
  ["A request Ersa can't show", { state: "unknown" }],
  // A request whose flags didn't arrive: the boundary check stops these, and the shelf's own boundary is the last guard.
  ["A request that can't be drawn", { state: "permission", request: { ...shelfRequest, flags: undefined as never } }],
  ["Keyboard focus on the shelf itself", { state: "permission", request: shelfRequest }, { focus: true }],
  ["An answer on its way", { state: "permission", request: shelfRequest }, { answer: { state: "sending", decision: "allow" } }],
  ["An answer refused", { state: "permission", request: shelfRequest }, { answer: { state: "failed", message: "the session has ended" } }],
  ["While Ersa reconnects", { state: "permission", request: shelfRequest }, { live: false }],
  ["Risen to fit a short window", { state: "permission", request: shelfRequest }, { short: true }],
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
  /** Shown on the rail's own material. */
  rail?: boolean;
}

const entries: Entry[] = [
  {
    name: "RailTop",
    rail: true,
    states: [
      ["At rest", <RailTop onNewSession={() => {}} />],
      ["Pointer over it", <RailTop onNewSession={() => {}} force="hover" />],
      ["Pressed", <RailTop onNewSession={() => {}} force="pressed" />],
      ["Keyboard focus", <RailTop onNewSession={() => {}} force="focus" />],
      ["Before Ersa connects", <RailTop onNewSession={() => {}} disabled />],
    ],
  },
  {
    name: "RailFoot",
    states: coreViews.map(([label, view]) => [
      label,
      view.phase === "failed" ? <RetryingFoot view={view} /> : <RailFoot view={view} onRetry={() => {}} />,
    ]),
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
        <QuitQuestion busy={2} onQuit={() => {}} onCancel={() => {}} takeFocus={false} failed="Ersa did not quit: Ersa isn't connected." />,
      ],
    ],
  },
  {
    name: "RailTree",
    rail: true,
    states: [
      ["Loading", <RailTree {...tree} projects={{ state: "loading" }} sessions={{ state: "loading" }} />],
      ["No projects", <RailTree {...tree} projects={{ state: "ready", projects: [] }} sessions={{ state: "ready", sessions: [] }} />],
      ["Projects", <RailTree {...tree} />],
      // ersa holds a session waiting on a permission request; runner's sessions want nothing.
      ["Collapsed", <RailTree {...tree} collapsed={folded("p1", "p5")} />],
      ["Collapsed, a question", <RailTree {...askingOnly} collapsed={folded("p1")} />],
      [
        "Collapsed, not trusted",
        <RailTree {...ersaOnly} projects={{ state: "ready", projects: [{ ...projects[0]!, trusted: false }] }} collapsed={folded("p1")} />,
      ],
      [
        "Collapsed, not trusted, keyboard focus",
        <RailTree
          {...ersaOnly}
          projects={{ state: "ready", projects: [{ ...projects[0]!, trusted: false }] }}
          collapsed={folded("p1")}
          forceHeading={{ id: "p1", state: "focus" }}
        />,
      ],
      ["Collapsed, pointer over it", <RailTree {...ersaOnly} collapsed={folded("p1")} forceHeading={{ id: "p1", state: "hover" }} />],
      ["Collapsed, keyboard focus", <RailTree {...ersaOnly} collapsed={folded("p1")} forceHeading={{ id: "p1", state: "focus" }} />],
      ["A session with no project", <RailTree {...ersaOnly} sessions={{ state: "ready", sessions: treeSessions }} />],
      ["Projects not read", <RailTree {...tree} projects={{ state: "error", message: "the database is locked" }} />],
      ["Sessions not read", <RailTree {...tree} sessions={{ state: "error", message: "the database is locked" }} />],
      ["Workspaces not read", <RailTree {...tree} workspaces={{ w1: { project_id: "p1" } }} workspaceErrors={{ p5: "the host devbox did not answer" }} />],
      ["Can't reach the sessions", <RailTree {...tree} projects={{ state: "unavailable" }} sessions={{ state: "unavailable" }} />],
      ["Heading, pointer over it", <RailTree {...ersaOnly} forceHeading={{ id: "p1", state: "hover" }} />],
      ["Heading, pressed", <RailTree {...ersaOnly} forceHeading={{ id: "p1", state: "pressed" }} />],
      ["Heading, keyboard focus", <RailTree {...ersaOnly} forceHeading={{ id: "p1", state: "focus" }} />],
      ["More, trusted", room(<RailTree {...ersaOnly} initialMenu="p1" />)],
      ["More, while trust is saved", room(<RailTree {...ersaOnly} initialMenu="p1" trusting={{ p1: { state: "sending" } }} />)],
      ["More, not trusted", room(<RailTree {...strangerOnly} initialMenu="p4" />)],
      ["Asking to trust", <RailTree {...strangerOnly} initialAsking="p4" />],
      ["Saving trust", <RailTree {...ersaOnly} trusting={{ p1: { state: "sending" } }} />],
      [
        "Trust withdrawn, sessions running",
        <RailTree {...ersaOnly} projects={{ state: "ready", projects: [{ ...projects[0]!, trusted: false }] }} keepingTrust={["p1"]} />,
      ],
      ["Trust refused", <RailTree {...ersaOnly} trusting={{ p1: { state: "failed", message: "no project p1" } }} />],
      ["Every tile state", <RailTree {...ersaOnly} sessions={{ state: "ready", sessions: everyState }} activeId={undefined} />],
      ["Tile, pointer over it", <RailTree {...pairOnly} force={{ id: "p2", state: "hover" }} />],
      ["Tile, pressed", <RailTree {...pairOnly} force={{ id: "p2", state: "pressed" }} />],
      ["Tile, keyboard focus", <RailTree {...pairOnly} force={{ id: "p2", state: "focus" }} />],
      // p1 finished a turn while p2 was active: its title reads strong until it is opened.
      ["Unseen", <RailTree {...pairOnly} activeId="p2" unseen={new Set(["p1"])} />],
      ["Active tile", <RailTree {...pairOnly} activeId="p2" />],
      ["Active tile, pointer over it", <RailTree {...pairOnly} activeId="p2" force={{ id: "p2", state: "hover" }} />],
      ["Active tile, pressed", <RailTree {...pairOnly} activeId="p2" force={{ id: "p2", state: "pressed" }} />],
      ["Active tile, keyboard focus", <RailTree {...pairOnly} activeId="p2" force={{ id: "p2", state: "focus" }} />],
    ],
  },
  {
    name: "Shelf",
    wide: true,
    states: shelfStates.map(([label, shows, more]) => [label, <ShelfOpen shows={shows} more={more} />]),
  },
  {
    name: "Transcript",
    wide: true,
    states: transcriptStates.map(([label, view, answers]) => [
      label,
      <Transcript title="Fix the flaky reconnect test" view={view} answers={answers} onDecide={() => {}} onRetry={() => {}} />,
    ]),
  },
  {
    name: "SessionTitle",
    wide: true,
    states: [
      ["Default", <Titled />],
      ["Hover", <Titled force="hover" />],
      ["Focus", <Titled force="focus" />],
      ["Core not connected", <Titled disabled />],
      ["Editing", <Titled initialDraft={draft} />],
      ["Editing, hover", <Titled initialDraft={draft} force="hover" />],
      ["Editing, focus", <Titled initialDraft={draft} force="focus" />],
      ["Blank", <Titled initialDraft="" force="focus" />],
      ["Saving", <Titled initialDraft={draft} renaming={{ state: "sending" }} />],
      ["Refused", <Titled initialDraft={draft} renaming={{ state: "failed", message: "the backend has no record of this session to rename" }} />],
    ],
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
    name: "NewSessionScreen",
    wide: true,
    states: [
      // FLOW-NEW-SESSION-R19: with no project the chip offers to choose a folder, and its list holds Add project… alone.
      ["No project", <NewSessionScreen {...newSession} projects={[]} backends={oneBackend} />],
      ["No project, the project chip open", room(<NewSessionScreen {...newSession} projects={[]} backends={oneBackend} open="project" />)],
      // FLOW-NEW-SESSION-R27: the trust question above the box, holding the focal glow.
      ["Asking to trust a folder", <NewSessionScreen {...newSession} backends={oneBackend} initialAsking="D:\Clones\stranger" />],
      [
        "Asking, from the keyboard",
        <NewSessionScreen {...newSession} backends={oneBackend} initialAsking="D:\Clones\stranger" forceCancelFocus />,
      ],
      [
        "Adding a folder refused",
        <NewSessionScreen {...newSession} backends={oneBackend} adding={{ state: "failed", message: "D:\\Projects\\nowhere does not exist" }} />,
      ],
      ["Ready, at the defaults", <NewSessionScreen {...newSession} backends={oneBackend} forceFocus />],
      ["Project without version control", <NewSessionScreen {...newSession} backends={oneBackend} projectId="p3" />],
      ["Backend missing, beside one available", room(<NewSessionScreen {...newSession} open="backend" />)],
      ["No backend available", <NewSessionScreen {...newSession} backends={{ local: [{ ...claudeCode, status: "error" }] }} />],
      ["Detecting backends", <NewSessionScreen {...newSession} backends={{}} />],
      ["A chip open", room(<NewSessionScreen {...newSession} backends={oneBackend} open="permission_mode" />)],
      // FLOW-NEW-SESSION-R14: most recently used first, with path, kind and trust, the chosen one marked.
      ["The project chip open", room(<NewSessionScreen {...newSession} backends={oneBackend} projectId="p2" open="project" />)],
      ["The workspace chip open", tallRoom(<NewSessionScreen {...newSession} backends={oneBackend} open="workspace" />)],
      [
        "Starting",
        <NewSessionScreen
          {...newSession}
          backends={oneBackend}
          initialText="Find why the reconnect test is flaky."
          starting={{ state: "sending" }}
        />,
      ],
      [
        "Start refused",
        <NewSessionScreen
          {...newSession}
          backends={oneBackend}
          initialText="Find why the reconnect test is flaky."
          starting={{ state: "failed", message: "Claude Code is not logged in. Run claude and log in, then try again." }}
        />,
      ],
    ],
  },
  {
    name: "Chip",
    // On the canvas, as the session controls stand, so the hover fill shows as it does on the screen.
    wide: true,
    states: [
      ["Default", <Chip {...chip} />],
      ["Hover", <Chip {...chip} force="hover" />],
      ["Focus", <Chip {...chip} force="focus" />],
      ["Pressed", <Chip {...chip} force="pressed" />],
      ["Disabled", <Chip {...chip} disabled />],
      ["With a prefix", <Chip name="Effort" prefix="Effort" value="high" options={[{ id: "high", label: "High" }]} onChoose={() => {}} />],
      ["A problem", <Chip name="Backend" placeholder="No backend" problem options={[]} onChoose={() => {}} />],
      // Under a running session: the session's value, without the chevron, unchanged under the pointer.
      ["In a session", <Chip {...chip} readOnly />],
      ["Detecting backends", <Chip name="Backend" placeholder="Detecting backends…" pending options={[]} onChoose={() => {}} disabled />],
      // Where the window has no room beneath, the list opens above its chip.
      ["Open above", <div className="gallery-room-before">{<Chip {...chip} initialOpen forceAbove />}</div>],
      ["Open", room(<Chip {...chip} initialOpen />)],
      ["Open, an option hovered", room(<Chip {...chip} initialOpen forceOption={{ id: "root", state: "hover" }} />)],
      ["Open, an option pressed", room(<Chip {...chip} initialOpen forceOption={{ id: "root", state: "pressed" }} />)],
      ["Open, two lines and an action", room(<Chip {...projectChip} forceOption={{ id: "p2", state: "hover" }} />)],
      // The action as it will be once it can be taken, with its shortcut.
      ["Open, the action available", room(<Chip {...projectChip} action={{ ...projectChip.action, onChoose: () => {} }} />)],
      ["Reads its choice", <Chip {...chip} reads="New workspace from main" />],
      ["Open, a section to search", tallRoom(<Chip {...chip} reads="New workspace from main" section={baseSection(branchOptions)} initialOpen />)],
      [
        "Open, more branches than fit",
        tallRoom(
          <Chip
            {...chip}
            reads="New workspace from main"
            section={baseSection([...branchOptions, ...["a", "b", "c", "d", "e"].map((n) => ({ id: `topic/${n}`, label: `topic/${n}`, status: undefined }))])}
            initialOpen
          />,
        ),
      ],
      [
        "Open, the section narrowed",
        tallRoom(<Chip {...chip} reads="New workspace from main" section={baseSection(branchOptions)} initialQuery="feat" initialOpen />),
      ],
      [
        "Open, nothing in the section matches",
        tallRoom(<Chip {...chip} reads="New workspace from main" section={baseSection(branchOptions)} initialQuery="hotfix" initialOpen />),
      ],
      [
        "Open, the section being read",
        tallRoom(<Chip {...chip} section={{ ...baseSection([], "Reading the branches…"), unavailable: true }} initialOpen />),
      ],
      [
        "Open, the section unreadable",
        tallRoom(<Chip {...chip} section={{
              ...baseSection([], "Ersa couldn't read this project's branches. The new workspace will start at the default branch."),
              unavailable: true,
              problem: true,
            }} initialOpen />),
      ],
      [
        "Open, with statuses",
        room(
          <Chip
            name="Backend"
            value="claude-code"
            options={[
              { id: "claude-code", label: "Claude Code", status: "2.1.302" },
              { id: "copilot", label: "GitHub Copilot", status: "Not installed", unavailable: true },
              { id: "codex", label: "codex", status: "Detection failed", failed: true, unavailable: true },
            ]}
            onChoose={() => {}}
            initialOpen
          />,
        ),
      ],
      [
        "Open, an unavailable option under the pointer",
        room(
          <Chip
            name="Backend"
            value="claude-code"
            options={[
              { id: "claude-code", label: "Claude Code", status: "2.1.302" },
              { id: "copilot", label: "GitHub Copilot", status: "Not installed", unavailable: true },
            ]}
            onChoose={() => {}}
            initialOpen
            forceOption={{ id: "copilot", state: "hover" }}
          />,
        ),
      ],
    ],
  },
  {
    name: "ContextRing",
    states: [
      ["Nothing used yet", <ContextRing />],
      ["Some used", <ContextRing used={0.42} />],
      ["Nearly full", <ContextRing used={0.93} />],
      ["On a tile", <ContextRing used={0.42} size="ring-sm" />],
      ["On a tile, nearly full", <ContextRing used={0.93} size="ring-sm" />],
    ],
  },
  {
    name: "Composer",
    wide: true,
    states: [
      ["Empty", <Composer autoFocus={false} onSend={async () => false} />],
      [
        "Under a session, with its values",
        <Composer
          autoFocus={false}
          onSend={async () => false}
          controls={
            <SessionControls
              hosts={[hello.host]}
              localHostId={hello.host.id}
              projects={projects}
              backends={[claudeCode]}
              values={{ host_id: hello.host.id, project_id: "p1", workspace: "isolated", backend: "claude-code", model: "opus", effort: "high", permission_mode: "default" }}
              workspaceLabel="ersa/fix-reconnect"
              onChoose={() => {}}
              readOnly
              used={0.42}
            />
          }
        />,
      ],
      ["Hover", <Composer autoFocus={false} onSend={async () => false} force="hover" />],
      ["Send under the pointer", <Composer autoFocus={false} onSend={async () => false} initialText="Run the tests again." forceSend="hover" />],
      ["Send pressed", <Composer autoFocus={false} onSend={async () => false} initialText="Run the tests again." forceSend="pressed" />],
      ["Focus", <Composer autoFocus={false} onSend={async () => false} force="focus" />],
      // A permission request holds the screen's focal glow, so the box shows focus by its edge alone.
      ["Focus, a permission request open", <Composer autoFocus={false} onSend={async () => false} force="focus" quietFocus />],
      ["Typed", <Composer autoFocus={false} onSend={async () => false} initialText="Run the tests again, then summarise what failed." />],
      ["Sending", <Composer autoFocus={false} onSend={async () => false} initialText="Run the tests again." sending={{ state: "sending" }} />],
      [
        "Refused",
        statusRoom(
          <Composer autoFocus={false} onSend={async () => false} initialText="Run the tests again." sending={{ state: "failed", message: refusal }} />,
        ),
      ],
      [
        "Can't reach the sessions, trying again",
        statusRoom(<Composer autoFocus={false} onSend={async () => false} initialText="Run the tests again." unavailable="reconnecting" />),
      ],
      ["Can't reach the sessions", statusRoom(<Composer autoFocus={false} onSend={async () => false} unavailable="unreachable" />)],
      ["Detecting backends", statusRoom(<Composer autoFocus={false} onSend={async () => false} unavailable="detecting" />)],
      ["No backend", statusRoom(<Composer autoFocus={false} onSend={async () => false} unavailable="no-backend" />)],
      ["A backend not working", statusRoom(<Composer autoFocus={false} onSend={async () => false} unavailable={{ failedBackend: "Claude Code" }} />)],
    ],
  },
  {
    name: "Icon",
    states: [
      ["At icon, beside ui type", <span className="gallery-icon-row"><Icon icon={Plus} /> New session</span>],
      ["At icon-sm, beside ui-small type", <span className="gallery-icon-row"><Icon icon={ChevronDown} size="icon-sm" /> ersa <Icon icon={ChevronRight} size="icon-sm" /></span>],
      ["At icon-xs, a mark in a tile", <span className="gallery-icon-row"><Icon icon={ChevronRight} size="icon-xs" /></span>],
      ["A mark that carries state, in its tone", <span className="gallery-icon-row"><span className="gallery-icon-caution"><Icon icon={ShieldAlert} size="icon-sm" /></span> Not trusted</span>],
      ["Icon button", <IconButton icon={X} label="Close" />],
      ["Icon button hover", <IconButton icon={X} label="Close" force="hover" />],
      ["Icon button focus", <IconButton icon={X} label="Close" force="focus" />],
      ["Icon button pressed", <IconButton icon={X} label="Close" force="pressed" />],
      ["Icon button disabled", <IconButton icon={X} label="Close" disabled />],
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

// Narrows the gallery to one component and/or one theme. The screenshot tests
// photograph each component on its own page, so a change in one component's
// height cannot move another component's section and change its pixels.
export type GalleryFilter = { component?: string | null; theme?: string | null };

export function Gallery({ only = {} }: { only?: GalleryFilter }) {
  const themes = (["light", "dark"] as const).filter((theme) => !only.theme || theme === only.theme);
  const shown = entries.filter((entry) => !only.component || entry.name === only.component);
  return (
    <div className="gallery">
      {themes.map((theme) => (
        <div key={theme} className="gallery-theme" data-theme={theme}>
          {shown.map((entry) => (
            <section key={entry.name} className="gallery-entry" data-component={entry.name}>
              <div className="gallery-heading">
                <h2 className="gallery-name">{entry.name}</h2>
                <span className="gallery-theme-name">{theme}</span>
              </div>
              <div className={["gallery-states", entry.wide && "gallery-states-wide", entry.rail && "gallery-states-rail"].filter(Boolean).join(" ")}>
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
