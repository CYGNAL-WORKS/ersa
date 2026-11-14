// The development-only gallery (ADR 0008): every component in every state,
// light and dark side by side. The screenshot tests in e2e/ photograph each
// component section in each theme, so a visual change shows up in review.
// A component built for the window gets an entry here in the same change.

import { ChevronDown, ChevronRight, Plus, ShieldAlert, X } from "lucide-react";
import { type CSSProperties, type ReactNode, useEffect, useId, useState } from "react";
import { t } from "../catalog";
import { Button } from "../components/Button";
import { Chip } from "../components/Chip";
import { ContextRing } from "../components/ContextRing";
import { ArchivedNote } from "../components/ArchivedNote";
import { AgentLink, AgentText, CodeBlock } from "../components/AgentText";
import { AttachmentChip } from "../components/AttachmentChip";
import { Composer } from "../components/Composer";
import { DeleteQuestion } from "../components/DeleteQuestion";
import type { Attached } from "../session/attachments";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { Icon, IconButton } from "../components/Icon";
import { NewSessionScreen } from "../components/NewSessionScreen";
import { QuitQuestion } from "../components/QuitQuestion";
import { RailFoot } from "../components/RailFoot";
import { Menu, type MenuEntry } from "../components/Menu";
import { RailTop } from "../components/RailTop";
import { SessionsPage } from "../components/SessionsPage";
import { PINNED_KEY, RailTree } from "../components/RailTree";
import { SessionControls } from "../components/SessionControls";
import { Shelf } from "../components/Shelf";
import { SessionTitle } from "../components/SessionTitle";
import { QuestionForm } from "../components/QuestionForm";
import { ThinkingRow, ToolCard, type ToolContext } from "../components/ToolCard";
import { Transcript } from "../components/Transcript";
import type { CoreView, SessionsView } from "../core/connection";
import type { Envelope, Event, QuestionAsked } from "../protocol/agent-events.gen";
import type { BackendInfo, Choice, CoreHelloResult, Project, Session, Workspace, WorkspaceChanges } from "../protocol/core-api.gen";
import type { Answer } from "../session/answers";
import { branchesMatching } from "../session/controls";
import { type DeletePhase, whatGoes } from "../session/deletion";
import type { SessionView } from "../session/follow";
import type { Draft } from "../session/questionForm";
import { type ShelfShows, shelfTone, shownRequest } from "../session/shelf";
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
  archived: false,
  pinned: false,
  unseen: false,
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
  // The last title a tile saved, kept on the page as above.
  onRename: async (id, title) => {
    document.body.dataset.railCall = `rename ${id} ${title}`;
    return true;
  },
  onPin: (id, pinned) => (document.body.dataset.railCall = `pin ${id} ${pinned}`),
  onArchive: (id, archived) => (document.body.dataset.railCall = `archive ${id} ${archived}`),
  onDelete: (id) => (document.body.dataset.railCall = `delete ${id}`),
  localHost: "local",
  hostNames: { local: "WORKSTATION", devbox: "devbox" },
};
/** An archived session's tile above a resting one, as the rail shows them showing all sessions (docs/design/RAIL.md, An archived tile). */
const archivedPair: Parameters<typeof RailTree>[0] = {
  ...tree,
  projects: { state: "ready", projects: [projects[0]!] },
  sessions: { state: "ready", sessions: [{ ...pointerPair[0]!, id: "a1", title: "Retire the old protocol notes", archived: true }, pointerPair[1]!] },
  activeId: undefined,
  show: "all",
};

const galleryNow = new Date(2026, 10, 7, 15);

/** The sessions page's rows, on 7 November 2026: each kind of mark, a remote host's, and every kind of time. */
const minutesBefore = (minutes: number) => new Date(galleryNow.getTime() - minutes * 60_000).toISOString();
const pageSessions: Session[] = [
  session("r1", "Fix the flaky reconnect test", "2026-11-01T09:00:00Z", { state: "waiting_user", pending: [{ kind: "permission", request_id: "r1" }] }),
  session("r2", "Tidy the gallery", "2026-11-01T08:00:00Z", { state: "running", pending: [] }),
  session("r3", "A title long enough that the row has to cut it with an ellipsis before it reaches the project and the time", "2026-11-01T07:00:00Z", { state: "idle", pending: [] }),
  { ...session("r4", "Port the runner", "2026-11-01T06:00:00Z", { state: "error", pending: [] }), workspace_id: "w-dev", host_id: "devbox" },
  session("r5", "Write the release notes", "2026-11-01T05:00:00Z", { state: "ended", pending: [] }),
].map((s, i) => ({ ...s, last_activity_at: minutesBefore([0, 5, 180, 60 * 24, 60 * 24 * 6][i] ?? 0) }));
const sessionsPage: Parameters<typeof SessionsPage>[0] = {
  sessions: { state: "ready", sessions: pageSessions },
  projects: treeProjects,
  workspaces: treeWorkspaces,
  localHost: "local",
  hostNames: { local: "WORKSTATION", devbox: "devbox" },
  now: galleryNow,
  onOpen: () => {},
  onNewSession: () => {},
  onRename: async () => true,
  onPin: () => {},
  onArchive: () => {},
  onDelete: () => {},
};
/** The marks the other rows don't show. */
const pageMarks: SessionsView = {
  state: "ready",
  sessions: [
    session("m1", "Asking a question", "2026-11-01T09:00:00Z", { state: "waiting_user", pending: [{ kind: "question", request_id: "q1" }] }),
    session("m2", "Starting up", "2026-11-01T08:00:00Z", { state: "starting", pending: [] }),
    session("m3", "Lost its connection", "2026-11-01T07:00:00Z", { state: "disconnected", pending: [] }),
    { ...session("m4", "Being deleted", "2026-11-01T06:00:00Z", { state: "ended", pending: [] }), deleting: "running" },
    { ...session("m5", "Could not be deleted", "2026-11-01T05:00:00Z", { state: "ended", pending: [] }), deleting: "failed" },
  ],
};
const pageArchived: SessionsView = { state: "ready", sessions: pageSessions.map((s) => ({ ...s, archived: true })) };

/** The tree grouped by date, on 7 November 2026: ersa's and runner's sessions, made across the last fortnight. */
const daysAgo = (days: number, hour: number) => new Date(2026, 10, 7 - days, hour).toISOString();
const datedTree: Parameters<typeof RailTree>[0] = {
  ...tree,
  groupBy: "date",
  now: galleryNow,
  sessions: {
    state: "ready",
    sessions: treeSessions.map((s, i) => ({ ...s, created_at: daysAgo([0, 1, 3, 0, 12][i] ?? 0, 9 - i) })),
  },
};

/** The rail's top row as the window draws it, with the arrange button beside New session. */
const arrange = { arrangement: { groupBy: "project", sortBy: "created", show: "active" }, onGroupBy: () => {}, onSortBy: () => {}, onShow: () => {}, onReset: () => {}, onAllSessions: () => {} } as const;

/** One project, for the states of a heading and its tiles. */
const ersaOnly: Parameters<typeof RailTree>[0] = {
  ...tree,
  projects: { state: "ready", projects: [projects[0]!] },
  sessions: { state: "ready", sessions: treeSessions.filter((s) => s.workspace_id === "w1") },
};
const strangerOnly: Parameters<typeof RailTree>[0] = { ...tree, projects: { state: "ready", projects: [projects[3]!] }, sessions: { state: "ready", sessions: [] } };
/** A resting tile above each forced one, so the two read apart in the screenshot. */
const pairOnly: Parameters<typeof RailTree>[0] = { ...ersaOnly, sessions: { state: "ready", sessions: pointerPair }, activeId: undefined };
/** One session being deleted and one whose deletion failed. */
// Core stops a session before it deletes anything, so both are stopped.
const beingDeleted: Session[] = pointerPair.map((s, i) => ({ ...s, tile: { ...s.tile, state: "idle" }, deleting: i === 0 ? "running" : "failed" }));
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
/** The tree with s1 and s4 pinned: they lead it in the Pinned group, apart from ersa and runner. */
const pinnedTree: Parameters<typeof RailTree>[0] = {
  ...tree,
  sessions: { state: "ready", sessions: treeSessions.map((s) => (s.id === "s1" || s.id === "s4" ? { ...s, pinned: true } : s)) },
};

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
// FLOW-SEND-MESSAGE-R11: a message with an image, a file and a folder, then one of a file alone. The image is a
// drawn mockup, wider than tall, so it shows at the largest size either way.
const mockup = btoa(
  '<svg xmlns="http://www.w3.org/2000/svg" width="480" height="300"><rect width="480" height="300" fill="lavender"/>' +
    '<rect x="24" y="24" width="120" height="252" rx="8" fill="lightsteelblue"/><rect x="168" y="24" width="288" height="40" rx="8" fill="white"/>' +
    '<rect x="168" y="236" width="288" height="40" rx="8" fill="royalblue"/></svg>',
);
const withAttachments: Event[] = [
  {
    type: "user.message",
    origin: "human",
    text: "Make the composer match this mockup.",
    attachments: [
      { kind: "image", media_type: "image/svg+xml", data: mockup },
      { kind: "file", path: "docs/design/COMPOSER.md" },
      { kind: "folder", path: "apps/shell/src/components" },
    ],
  },
  { type: "assistant.text", block_id: "m1/0", text: "I'll start with the attachments above the words.", final: true },
  { type: "user.message", origin: "human", text: "", attachments: [{ kind: "file", path: "docs/design/DESIGN.md" }] },
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

// What the user sees of Core's refusal that names no reason: its message without the method name (describeError). A
// refusal with a reason reads in the catalog's words (errors.refusal).
const refusal = "the session is a replay of a recording and cannot change";

// Questions as Claude Code asks them (PROTO-AGENT-EVENTS-R18): options with descriptions, and own words beside them.
type Question = QuestionAsked["questions"][number];
const retryOptions = [
  { id: "o1", label: "Windows only", description: "Where the test flakes today" },
  { id: "o2", label: "Everywhere", description: "Linux and macOS runners too" },
  { id: "o3", label: "Nowhere", description: "Fix the race instead" },
];
const singleQuestion: Question = { id: "q1", header: "Retry", text: "Where should the reconnect test retry?", options: retryOptions, multi_select: false, takes_text: true };
const multiQuestion: Question = {
  id: "q2",
  header: "Runners",
  text: "Which runners should run the reconnect test?",
  options: [
    { id: "w", label: "Windows" },
    { id: "l", label: "Linux" },
    { id: "m", label: "macOS", description: "Slowest to start" },
  ],
  multi_select: true,
  takes_text: true,
};
const wordsQuestion: Question = { id: "q3", text: "What should the new branch be called?", multi_select: false, takes_text: false };
const questionAsked: Event[] = [
  { type: "session.state", state: "waiting_user" },
  { type: "question.asked", request_id: "rq", questions: [singleQuestion] },
];

// The agent's words with every construct the transcript formats (docs/design/TRANSCRIPT.md, Formatted text).
const formatted = [
  "## What changed",
  "",
  "The reconnect test was **flaky** because the fake Core answered *before* the window subscribed. I moved the",
  "subscription ahead of the handshake in `connection.ts`; the ~~sleep~~ is gone.",
  "",
  "```ts",
  "const events = new Channel<CoreEvent>();",
  "events.onmessage = onEvent;",
  "return invoke<Attachment>(\"core_attach\", { events });",
  "```",
  "",
  "### Checks",
  "",
  "- [x] `pnpm test` passes",
  "- [ ] The live check, once Core is built",
  "",
  "1. Start Core.",
  "2. Open a session and drop the pipe.",
  "",
  "> The window must hear every event Core sends during the attach.",
  "",
  "| Run | Before | After |",
  "| --- | --- | --- |",
  "| Windows | 3 of 10 failed | 0 of 50 failed |",
  "| Linux | 1 of 10 failed | 0 of 50 failed |",
  "",
  "---",
  "",
  "See [the bridge notes](https://example.com/bridge) or write to team@example.com. The diagram is ![the attach sequence](https://example.com/attach.png).",
  "Raw HTML such as <b>this</b> shows as its text, and [a file link](file:///C:/notes.txt) as plain words.",
].join("\n");

const formattedStreaming = ["Here is the fix:", "", "```rust", "fn reconnect(&mut self) {", "    self.subscribe();"].join("\n");

const transcriptStates: [string, SessionView, answers?: Record<string, Answer>][] = [
  ["Loading", sessionView([], { stream: { state: "subscribing" } })],
  ["Empty", sessionView([])],
  ["Streaming", sessionView(streaming)],
  ["Permission open", sessionView(asked)],
  ["Permission sending", sessionView(asked), { r1: { state: "sending", decision: "allow" } }],
  ["Permission refused", sessionView(asked), { r1: { state: "failed", message: refusal } }],
  ["Permission while Core is away", sessionView(asked, { stream: { state: "offline" } })],
  ["Question open", sessionView(questionAsked)],
  ["Question answer on its way", sessionView(questionAsked), { rq: { state: "sending", decision: "decline" } }],
  ["Question answer refused", sessionView(questionAsked), { rq: { state: "failed", message: refusal } }],
  ["Question while Core is away", sessionView(questionAsked, { stream: { state: "offline" } })],
  ["Question answered", sessionView([...questionAsked, { type: "question.answered", request_id: "rq", answers: [{ id: "q1", value: "o2", own_words: false }], declined: false, answered_by: "user" }])],
  ["Question declined", sessionView([...questionAsked, { type: "question.answered", request_id: "rq", answers: [], declined: true, answered_by: "user" }])],
  ["Question not answered", sessionView([...questionAsked, { type: "session.ended", reason: "backend_exited" }])],
  ["Hidden characters", sessionView(hiding)],
  ["Answered", sessionView(answered)],
  ["Formatted text", sessionView([{ type: "user.message", origin: "human", text: "What did you change?" }, { type: "assistant.text", block_id: "m1/0", text: formatted, final: true }])],
  ["Two turns", sessionView(twoTurns)],
  ["Long path", sessionView(longPath)],
  ["Attachments", sessionView(withAttachments)],
  ["Failed", sessionView(failing)],
  ["Not receiving events", sessionView([], { stream: { state: "failed", message: "Core closed the connection" } })],
];

// The transcript's events all carry one time; its running cards read the clock 12 seconds after it.
const transcriptNow = Date.parse("2026-11-01T09:00:12Z");

/** Transcript items built as the window builds them, each event at its own second. */
function itemsAt(steps: [seconds: number, Event][], input: "live" | "record" = "live") {
  return steps
    .map(
      ([seconds, event], seq): Envelope => ({
        v: EVENT_VERSION,
        session_id: "s1",
        seq,
        ts: new Date(Date.UTC(2026, 10, 1, 9, 0, seconds)).toISOString(),
        source: { backend: "claude-code", backend_version: "2.1.316", input },
        event,
      }),
    )
    .reduce(apply, emptyTranscript).items;
}
type ToolItem = Extract<ReturnType<typeof itemsAt>[number], { kind: "tool" }>;
type ThinkingItem = Extract<ReturnType<typeof itemsAt>[number], { kind: "thinking" }>;
// Running cards read the clock three minutes and five seconds after their calls began.
const toolNow = Date.UTC(2026, 10, 1, 9, 0, 0) + 185_000;
const toolWorkspace = "D:\\Projects\\ersa";
const designPage = "D:\\Projects\\ersa\\docs\\design\\TRANSCRIPT.md";
const readCall: Event = { type: "tool.requested", tool_use_id: "r1", name: "Read", input: { file_path: designPage, limit: 80 }, target: designPage };
const readDone: Event = { type: "tool.result", tool_use_id: "r1", content: "# The transcript", is_error: false, duration_ms: 2400 };
const bashCall: Event = {
  type: "tool.requested",
  tool_use_id: "b1",
  name: "Bash",
  input: { command: "cargo test -p ersa-core", description: "Run the core's tests" },
  target: "cargo test -p ersa-core",
};
const testOutput = "running 48 tests\ntest session::tests::resumes ... ok\ntest session::tests::ends ... ok\n\ntest result: ok. 48 passed; 0 failed";
const subagent: [number, Event][] = [
  [0, { type: "tool.requested", tool_use_id: "a1", name: "Agent", input: { description: "Count files", prompt: "Count the files in docs." }, target: "Count files" }],
  [1, { type: "tool.requested", tool_use_id: "g1", name: "Glob", input: { pattern: "docs/**/*.md" }, parent_tool_use_id: "a1", target: "docs/**/*.md" }],
  [2, { type: "tool.result", tool_use_id: "g1", content: "docs/GLOSSARY.md", is_error: false }],
  [3, { type: "tool.requested", tool_use_id: "g2", name: "Read", input: { file_path: "D:\\Projects\\ersa\\docs\\GLOSSARY.md" }, parent_tool_use_id: "a1", target: "D:\\Projects\\ersa\\docs\\GLOSSARY.md" }],
  [5, { type: "tool.result", tool_use_id: "g2", content: "# Glossary", is_error: false }],
  [8, { type: "tool.result", tool_use_id: "a1", content: "One file: docs/GLOSSARY.md.", is_error: false, duration_ms: 7804 }],
];
const longPathCall: Event = {
  type: "tool.requested",
  tool_use_id: "r1",
  name: "Read",
  input: {},
  target: "C:\\Users\\me\\AppData\\Local\\ersa\\target\\debug\\build\\ersa-core-5f3c2a9e1b7d4c08\\out\\generated_protocol_bindings.rs",
};
const longCommandCall: Event = {
  type: "tool.requested",
  tool_use_id: "b1",
  name: "Bash",
  input: {},
  target: "pnpm --filter shell exec vitest run src/components/ToolCard.dom.test.ts src/session/toolCard.test.ts --reporter verbose",
};

/** One call's card as the transcript shows it, with its subagent's calls, against the gallery's clock and workspace. */
function Card({
  steps,
  id = "r1",
  waiting,
  input,
  ...rest
}: { steps: [number, Event][]; id?: string; waiting?: boolean; input?: "live" | "record" } & Omit<Parameters<typeof ToolCard>[0], "item" | "context">) {
  const items = itemsAt(steps, input);
  const item = items.find((i): i is ToolItem => i.key === `tool:${id}`);
  if (!item) return null;
  const calls = new Map<string, ToolItem[]>();
  for (const i of items) if (i.kind === "tool" && i.parent) calls.set(i.parent, [...(calls.get(i.parent) ?? []), i]);
  const context: ToolContext = { awaiting: new Set(waiting ? [item.key] : []), calls, workspace: toolWorkspace, now: toolNow };
  return <ToolCard item={item} context={context} {...rest} />;
}
const thought = (final: boolean, summary?: string) =>
  itemsAt([[0, { type: "assistant.thinking", block_id: "m/0", final, ...(summary ? { summary } : {}) }]])[0] as ThinkingItem;
const thinkingSummary = "The reconnect test races the subscription. Check the order in connection.ts before touching the fake Core.";

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

// What `backend.list` lists for Claude Code (PROTO-CLAUDE-CODE-STREAM): the models the CLI reports, its own `default`
// entry first and one model that takes no effort, no effort marked, and the permission modes with the CLI's default.
const choices = (pairs: [string, string][], dflt: string): Choice[] =>
  pairs.map(([id, label]) => ({ id, label, default: id === dflt }));
const allEfforts = ["low", "medium", "high", "xhigh", "max"];
const claudeCode: BackendInfo = {
  name: "claude-code",
  label: "Claude Code",
  version: "2.1.316",
  status: "available",
  tested: { min: "2.1.0", max: "2.1.400" },
  models: [
    { id: "default", label: "Default (recommended)", default: true, resolved: "claude-opus-5-5", efforts: allEfforts },
    { id: "opus", label: "Opus 5.5", default: false, resolved: "claude-opus-5-5", efforts: allEfforts },
    { id: "claude-fable-5-2", label: "Fable 5.2", default: false, resolved: "claude-fable-5-2", efforts: allEfforts },
    { id: "sonnet", label: "Sonnet 5.5", default: false, resolved: "claude-sonnet-5-5", efforts: allEfforts },
    { id: "haiku", label: "Haiku 5.5", default: false, resolved: "claude-haiku-5-5", efforts: allEfforts },
    { id: "claude-haiku-4-5-20251001", label: "Haiku 4.5", default: false, resolved: "claude-haiku-4-5-20251001" },
  ],
  efforts: choices([["low", "Low"], ["medium", "Medium"], ["high", "High"], ["xhigh", "Extra high"], ["max", "Max"]], ""),
  permission_modes: choices(
    [["default", "Ask before acting"], ["acceptEdits", "Accept edits"], ["plan", "Plan only"], ["auto", "Auto"]],
    "default",
  ),
};
const copilot = (status: BackendInfo["status"]): BackendInfo => ({
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
/** Room above a composer for its attach menu, which opens upward from attach. */
const menuRoom = (node: ReactNode) => <div className="gallery-room-before gallery-list-above">{node}</div>;
/** A message's attachments (FLOW-SEND-MESSAGE-R09, R17): a file in the workspace, an image, and a folder Core copies. */
const galleryWorkspace = "w-gallery";
const inWorkspace = { workspace: galleryWorkspace, copied: false };
const galleryAttachments: [Attached, Attached, Attached] = [
  { path: "D:/code/ersa/docs/briefs/FLOW-SEND-MESSAGE.md", name: "FLOW-SEND-MESSAGE.md", kind: "file", detail: "212 lines", copied: inWorkspace },
  { path: "C:/Pictures/composer-mockup.png", name: "composer-mockup.png", kind: "image", detail: "84 KB" },
  { path: "C:/design/icons", name: "icons", kind: "folder", detail: "Folder", copied: { workspace: galleryWorkspace, copied: true } },
];
const attachProps = { autoFocus: false, onSend: async () => false, onAttach: async () => [], workspace: galleryWorkspace } as const;

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

/** Every part a menu has (docs/design/RAIL.md, Menus), as the arrange and tile menus will use them. */
const menuParts: MenuEntry[] = [
  {
    kind: "submenu",
    label: "Group by",
    current: "Project",
    entries: [
      { kind: "item", label: "Project", checked: true },
      { kind: "item", label: "Section" },
      { kind: "item", label: "Date" },
      { kind: "item", label: "None" },
    ],
  },
  { kind: "submenu", label: "Sort by", current: "Date created", entries: [{ kind: "item", label: "Date created", checked: true }] },
  { kind: "rule" },
  { kind: "item", label: "Rename", keyHint: "F2" },
  { kind: "item", label: "Fork", unavailable: "Available once the reply finishes" },
  { kind: "rule" },
  { kind: "item", label: "Delete…", danger: true },
];

/** A menu shown open below a stand-in for the button that opens it. */
function MenuOpen({ entries, submenu }: { entries: MenuEntry[]; submenu?: number }) {
  const anchor = `--gallery-menu-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  return (
    <div className="gallery-room">
      <span className="gallery-menu-anchor" style={{ anchorName: anchor } as CSSProperties} />
      <Menu label="Menu" entries={entries} open place={{ anchor }} focus="none" onClose={() => {}} initialSubmenu={submenu} />
    </div>
  );
}

/** A question's form on an open question card, with `initial` already chosen or written. */
function QuestionCardOpen({ questions, initial }: { questions: Question[]; initial: Draft }) {
  const [draft, setDraft] = useState<Draft>(initial);
  return (
    <section className="transcript-card transcript-question" data-open>
      <QuestionForm questions={questions} draft={draft} onDraft={setDraft} live onAnswer={() => {}} />
    </section>
  );
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
              <Shelf id={domId} tile={tile} shows={shows} onRetry={() => {}} answers={answer && { [shownRequest(shows) ?? ""]: answer }} live={live} onDecide={() => {}} onAnswer={() => {}} force={focus ? "focus" : undefined} />
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
  ["A single-choice question", { state: "question", requestId: "rq", questions: [singleQuestion] }],
  ["A multiple-choice question", { state: "question", requestId: "rq", questions: [multiQuestion] }],
  ["A question in the user's own words", { state: "question", requestId: "rq", questions: [wordsQuestion] }],
  ["A question's answer on its way", { state: "question", requestId: "rq", questions: [singleQuestion] }, { answer: { state: "sending", decision: { answers: [] } } }],
  ["A question's answer refused", { state: "question", requestId: "rq", questions: [singleQuestion] }, { answer: { state: "failed", message: "the session has ended" } }],
  ["A question while Ersa reconnects", { state: "question", requestId: "rq", questions: [singleQuestion] }, { live: false }],
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

/** The delete question's states (docs/design/RAIL.md, The delete question), from what `workspace.changes` answers. */
const deleteWorkspace: Workspace = { id: "w1", project_id: "p1", kind: "isolated", path: "C:\\Users\\me\\.ersa\\workspaces\\ersa-7", vcs_ref: "ersa/fix-reconnect", state: "ready" };
const deleteRoot: Workspace = { ...deleteWorkspace, kind: "root", path: "C:\\Users\\me\\ersa", vcs_ref: "" };
const counted = (changes: Partial<WorkspaceChanges>, workspace = deleteWorkspace) =>
  whatGoes(workspace, { uncommitted: 0, ignored: 0, unshared: 0, stranded: 0, sessions: 1, unrecorded: false, token: "t", ...changes });
const deleteStates: [string, Exclude<DeletePhase, { kind: "deleted" }>][] = [
  ["Folder and branch go, with work lost", { kind: "asking", changed: false, goes: counted({ uncommitted: 3, ignored: 12, unshared: 1, stranded: 1 }) }],
  ["Branch kept, nothing lost", { kind: "asking", changed: false, goes: counted({ unshared: 4 }) }],
  ["Files stay", { kind: "asking", changed: false, goes: whatGoes(deleteRoot, undefined) }],
  ["Git lost the workspace's record", { kind: "asking", changed: false, goes: counted({ uncommitted: 2, unshared: 1, unrecorded: true }) }],
  ["Asked again", { kind: "asking", changed: true, goes: counted({ uncommitted: 4 }) }],
  ["Reading what would be lost", { kind: "reading" }],
  ["Deleting", { kind: "deleting", goes: counted({ uncommitted: 3 }) }],
  ["Deletion failed", { kind: "failed", reason: "The folder is in use by another program: C:\\Users\\me\\.ersa\\workspaces\\ersa-7" }],
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
    name: "Menu",
    states: [
      ["Its parts", <MenuOpen entries={menuParts} />],
      ["A submenu open", <MenuOpen entries={menuParts} submenu={0} />],
    ],
  },
  {
    name: "RailTop",
    rail: true,
    states: [
      ["At rest", <RailTop onNewSession={() => {}} arrange={arrange} />],
      ["Pointer over it", <RailTop onNewSession={() => {}} arrange={arrange} force="hover" />],
      ["Pressed", <RailTop onNewSession={() => {}} arrange={arrange} force="pressed" />],
      ["Keyboard focus", <RailTop onNewSession={() => {}} arrange={arrange} force="focus" />],
      ["Before Ersa connects", <RailTop onNewSession={() => {}} arrange={arrange} disabled />],
      ["Arrange menu, open", room(<RailTop onNewSession={() => {}} arrange={{ ...arrange, initialOpen: true }} />)],
      [
        "Arrange menu, Group by open",
        room(<RailTop onNewSession={() => {}} arrange={{ ...arrange, arrangement: { groupBy: "date", sortBy: "created", show: "all" }, initialOpen: true, initialSubmenu: 0 }} />),
      ],
      ["Arrange menu, Sort by open", room(<RailTop onNewSession={() => {}} arrange={{ ...arrange, arrangement: { groupBy: "project", sortBy: "name", show: "active" }, initialOpen: true, initialSubmenu: 1 }} />)],
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
    name: "DeleteQuestion",
    wide: true,
    states: deleteStates.map(([label, phase]) => [
      label,
      <DeleteQuestion title="Fix the flaky reconnect test" backend="Claude Code" phase={phase} onConfirm={() => {}} onTryAgain={() => {}} onCancel={() => {}} inline />,
    ]),
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
      ["Pinned", <RailTree {...pinnedTree} />],
      // The Pinned group holds s1, which waits on a permission request, so its count leads with the caution mark.
      ["Pinned, collapsed", <RailTree {...pinnedTree} collapsed={new Set([PINNED_KEY])} />],
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
      // s1 waits on a permission request and is not the active session: More sits before its chevron.
      ["Needs you, pointer over it", <RailTree {...ersaOnly} force={{ id: "s1", state: "hover" }} />],
      // s1 is the active session and waits on a permission request: its chevron stays a mark, and More sits before it.
      ["Active, needs you, pointer over it", <RailTree {...ersaOnly} activeId="s1" force={{ id: "s1", state: "hover" }} />],
      // p2 is working, so Delete is unavailable with its reason; p1 is idle, so it can be deleted.
      ["Tile menu, open", room(<RailTree {...pairOnly} initialTileMenu="p2" />)],
      ["Tile menu, Delete available", room(<RailTree {...pairOnly} initialTileMenu="p1" />)],
      // LAY-SHELL, Being deleted: Deleting while Core works on it, and Couldn't delete, whose menu holds Delete… alone.
      ["Being deleted, and a deletion that failed", room(<RailTree {...pairOnly} sessions={{ state: "ready", sessions: beingDeleted }} initialTileMenu="p2" />)],
      // Renaming, archiving and deleting go through Core, so while Ersa can't reach it all are unavailable, with the reason.
      ["Tile menu, offline", room(<RailTree {...pairOnly} initialTileMenu="p2" offline />)],
      ["An archived tile", <RailTree {...archivedPair} />],
      ["An archived tile, pointer over it", <RailTree {...archivedPair} force={{ id: "a1", state: "hover" }} />],
      ["Tile menu, archived", room(<RailTree {...archivedPair} initialTileMenu="a1" />)],
      ["Grouped by date", <RailTree {...datedTree} />],
      ["Grouped by date, collapsed", <RailTree {...datedTree} collapsed={new Set(["yesterday"])} />],
      ["Grouped by none", <RailTree {...pinnedTree} groupBy="none" />],
      ["Sorted by name", <RailTree {...pinnedTree} groupBy="none" sortBy="name" />],
      ["No active sessions", <RailTree {...tree} sessions={{ state: "ready", sessions: treeSessions.map((s) => ({ ...s, archived: true })) }} />],
      ["No archived sessions", <RailTree {...tree} show="archived" />],
      ["Arrangement not saved", <RailTree {...ersaOnly} arrangeProblem={{ kind: "save", message: "settings.set: the database is locked" }} />],
      [
        "Archive refused",
        <RailTree {...pairOnly} archiving={{ p2: { state: "failed", message: t("errors.refusal.sessionDeletingError") } }} />,
      ],
      ["Tile menu, pinned", room(<RailTree {...pairOnly} sessions={{ state: "ready", sessions: pointerPair.map((s) => ({ ...s, pinned: true })) }} initialTileMenu="p2" />)],
      ["Tile menu, while a pin is saved", room(<RailTree {...pairOnly} initialTileMenu="p2" pinning={{ p2: { state: "sending" } }} />)],
      ["A fork on its way", <RailTree {...pairOnly} forking={{ p1: { state: "sending" } }} />],
      [
        "Fork refused",
        <RailTree {...pairOnly} forking={{ p1: { state: "failed", message: t("errors.refusal.noFinishedReplyError") } }} />,
      ],
      ["Pin refused", <RailTree {...pairOnly} pinning={{ p2: { state: "failed", message: t("errors.refusal.sessionDeletingError") } }} />],
      ["Renaming on the tile", <RailTree {...pairOnly} initialTileDraft={{ id: "p2", draft: "The one in question, renamed" }} />],
      [
        "A title the backend refused",
        <RailTree
          {...pairOnly}
          initialTileDraft={{ id: "p2", draft: "The one in question, renamed" }}
          renaming={{ p2: { state: "failed", message: "Claude Code has no record of this session any more." } }}
        />,
      ],
    ],
  },
  {
    name: "SessionsPage",
    wide: true,
    states: [
      ["Active", <SessionsPage {...sessionsPage} />],
      ["Archived", <SessionsPage {...sessionsPage} sessions={pageArchived} initialTab="archived" />],
      ["Pointer over a row", <SessionsPage {...sessionsPage} force={{ id: "r2", state: "hover" }} />],
      ["Keyboard focus on a row", <SessionsPage {...sessionsPage} force={{ id: "r2", state: "focus" }} />],
      ["Pressed row", <SessionsPage {...sessionsPage} force={{ id: "r2", state: "pressed" }} />],
      ["Every other mark", <SessionsPage {...sessionsPage} sessions={pageMarks} />],
      ["A long project name", <SessionsPage {...sessionsPage} projects={treeProjects.map((p) => ({ ...p, name: "a-project-whose-name-runs-on-far-too-long" }))} />],
      ["A tab, pointer over it", <SessionsPage {...sessionsPage} forceTab={{ tab: "archived", state: "hover" }} />],
      ["A tab, pressed", <SessionsPage {...sessionsPage} forceTab={{ tab: "archived", state: "pressed" }} />],
      ["A tab, keyboard focus", <SessionsPage {...sessionsPage} forceTab={{ tab: "active", state: "focus" }} />],
      ["Search open", <SessionsPage {...sessionsPage} initialSearch="tidy" forceSearch />],
      ["A search that matches nothing", <SessionsPage {...sessionsPage} initialSearch="nothing like it" />],
      ["A row's menu, open", room(<SessionsPage {...sessionsPage} initialTileMenu="r2" />)],
      ["Renaming on a row", <SessionsPage {...sessionsPage} initialTileDraft={{ id: "r2", draft: "Tidy the gallery, renamed" }} />],
      ["Rename refused", <SessionsPage {...sessionsPage} initialTileDraft={{ id: "r2", draft: "Tidy the gallery, renamed" }} renaming={{ r2: { state: "failed", message: "session.set_title: the backend refused the title" } }} />],
      ["Archive refused", <SessionsPage {...sessionsPage} archiving={{ r2: { state: "failed", message: t("errors.refusal.sessionDeletingError") } }} />],
      ["Loading", <SessionsPage {...sessionsPage} sessions={{ state: "loading" }} />],
      ["Could not be read", <SessionsPage {...sessionsPage} sessions={{ state: "error", message: "session.list: the database is locked" }} />],
      ["While Ersa can't reach Core", <SessionsPage {...sessionsPage} sessions={{ state: "unavailable" }} offline />],
      ["No active sessions", <SessionsPage {...sessionsPage} sessions={{ state: "ready", sessions: [] }} />],
      ["No archived sessions", <SessionsPage {...sessionsPage} sessions={{ state: "ready", sessions: [] }} initialTab="archived" />],
    ],
  },
  {
    // A question's form with choices made, as the shelf and the transcript's question card hold it.
    name: "QuestionForm",
    states: [
      ["One option chosen", <QuestionCardOpen questions={[singleQuestion]} initial={{ q1: { chosen: ["o2"] } }} />],
      ["Several options chosen", <QuestionCardOpen questions={[multiQuestion]} initial={{ q2: { chosen: ["w", "m"] } }} />],
      ["Answered in the user's own words", <QuestionCardOpen questions={[singleQuestion]} initial={{ q1: { words: "Only on the nightly run" } }} />],
      ["Two questions, one answered", <QuestionCardOpen questions={[singleQuestion, wordsQuestion]} initial={{ q1: { chosen: ["o1"] } }} />],
    ],
  },
  {
    name: "Shelf",
    wide: true,
    states: shelfStates.map(([label, shows, more]) => [label, <ShelfOpen shows={shows} more={more} />]),
  },
  {
    // A file or folder in a message in the transcript (FLOW-SEND-MESSAGE-R11; DESIGN.md, Layout): its states, and its path as
    // the tooltip with keyboard focus.
    name: "AttachmentChip",
    wide: true,
    states: [
      [
        "A long path, its name cut and its path wrapped",
        room(
          <ul className="transcript-attachments">
            <AttachmentChip kind="file" name="generated_protocol_bindings_for_the_session_stream_and_core_api.rs" path="target/debug/build/ersa-core-6f3c2a9e1b7d4c0f/out/generated_protocol_bindings_for_the_session_stream_and_core_api.rs" force="focus" />
          </ul>,
        ),
      ],
      ["At rest", <ul className="transcript-attachments"><AttachmentChip kind="file" name="COMPOSER.md" path="docs/design/COMPOSER.md" /></ul>],
      ["Under the pointer", <ul className="transcript-attachments"><AttachmentChip kind="file" name="COMPOSER.md" path="docs/design/COMPOSER.md" force="hover" /></ul>],
      ["Pressed", <ul className="transcript-attachments"><AttachmentChip kind="file" name="COMPOSER.md" path="docs/design/COMPOSER.md" force="pressed" /></ul>],
      ["Keyboard focus, its path shown", room(<ul className="transcript-attachments"><AttachmentChip kind="file" name="COMPOSER.md" path="docs/design/COMPOSER.md" force="focus" /></ul>)],
      ["A folder", <ul className="transcript-attachments"><AttachmentChip kind="folder" name="components" path="apps/shell/src/components" /></ul>],
      ["An image the window can't show", <ul className="transcript-attachments"><AttachmentChip kind="image" name="Attached image" /></ul>],
    ],
  },
  {
    name: "AgentText",
    wide: true,
    states: [
      ["Every construct", <AgentText text={formatted} />],
      ["Streaming, its code block still open", <AgentText text={formattedStreaming} />],
      ["Copy under the pointer", <CodeBlock language="bash" text="pnpm test" force="hover" />],
      ["Copy pressed", <CodeBlock language="bash" text="pnpm test" force="pressed" />],
      ["Copy with keyboard focus", <CodeBlock language="bash" text="pnpm test" force="focus" />],
      ["Copied", <CodeBlock language="bash" text="pnpm test" copied />],
      ["A block that names no language", <CodeBlock text="plain text" />],
      [
        "A link at rest, under the pointer, with keyboard focus",
        <p className="agent-text">
          <AgentLink href="https://example.com/a">At rest</AgentLink>, <AgentLink href="https://example.com/b" force="hover">under the pointer</AgentLink>,{" "}
          <AgentLink href="https://example.com/c" force="focus">with focus</AgentLink>
        </p>,
      ],
    ],
  },
  {
    name: "Transcript",
    wide: true,
    states: transcriptStates.map(([label, view, answers]) => [
      label,
      <Transcript title="Fix the flaky reconnect test" view={view} answers={answers} onDecide={() => {}} onAnswer={() => {}} onRetry={() => {}} now={transcriptNow} />,
    ]),
  },
  {
    name: "ToolCard",
    wide: true,
    states: [
      ["Running, its clock", <Card steps={[[0, bashCall]]} id="b1" />],
      ["Running, its progress", <Card steps={[[0, readCall], [1, { type: "tool.progress", tool_use_id: "r1", detail: "Reading TRANSCRIPT.md" }]]} />],
      ["Waiting for the user", <Card steps={[[0, bashCall]]} id="b1" waiting />],
      ["Done", <Card steps={[[0, readCall], [2, readDone]]} />],
      ["Under the pointer", <Card steps={[[0, readCall], [2, readDone]]} force="hover" />],
      ["Pressed", <Card steps={[[0, readCall], [2, readDone]]} force="pressed" />],
      ["Keyboard focus", <Card steps={[[0, readCall], [2, readDone]]} force="focus" />],
      ["Open", <Card steps={[[0, bashCall], [42, { type: "tool.result", tool_use_id: "b1", content: testOutput, is_error: false }]]} id="b1" initialOpen />],
      [
        "Failed, open",
        <Card steps={[[0, bashCall], [185, { type: "tool.result", tool_use_id: "b1", content: "error[E0425]: cannot find value `seq` in this scope", is_error: true }]]} id="b1" initialOpen />,
      ],
      ["A subagent's calls, open", <Card steps={subagent} id="a1" initialOpen />],
      // As narrow as a bubble, so the target overflows.
      [
        "Failed with no input and no output, open",
        <Card
          id="e1"
          initialOpen
          steps={[
            [0, { type: "tool.requested", tool_use_id: "e1", name: "mcp__docs__sync", input: {} }],
            [4, { type: "tool.result", tool_use_id: "e1", content: "", is_error: true }],
          ]}
        />,
      ],
      ["A long path, cut at its start", <div style={{ maxWidth: "var(--size-bubble-max)" }}><Card steps={[[0, longPathCall], [3, readDone]]} /></div>],
      ["A long command, cut at its end", <div style={{ maxWidth: "var(--size-bubble-max)" }}><Card steps={[[0, longCommandCall]]} id="b1" /></div>],
      ["Read from the record, unfinished", <Card steps={[[0, readCall]]} input="record" />],
    ],
  },
  {
    name: "ThinkingRow",
    wide: true,
    states: [
      ["Thinking", <ThinkingRow item={thought(false)} />],
      ["Thought", <ThinkingRow item={thought(true)} />],
      ["Thought, with a summary", <ThinkingRow item={thought(true, thinkingSummary)} />],
      ["Under the pointer", <ThinkingRow item={thought(true, thinkingSummary)} force="hover" />],
      ["Keyboard focus", <ThinkingRow item={thought(true, thinkingSummary)} force="focus" />],
      ["Open", <ThinkingRow item={thought(true, thinkingSummary)} initialOpen />],
    ],
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
      // docs/design/COMPOSER.md: with no level chosen the effort reads Default, the first row of its list.
      ["The effort chip open, at Default", room(<NewSessionScreen {...newSession} backends={oneBackend} open="effort" />)],
      // A model that takes no effort leaves the effort chip in its place, unavailable, reading None.
      [
        "A model that takes no effort",
        <NewSessionScreen
          {...newSession}
          backends={oneBackend}
          projectId="p1"
          lastUsed={{ p1: { host_id: "local", project_id: "p1", workspace: "isolated", backend: "claude-code", model: "claude-haiku-4-5-20251001", permission_mode: "default" } }}
        />,
      ],
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
          onStop={() => {}}
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
      ["Open above", <div className="gallery-room-before gallery-list-above">{<Chip {...chip} initialOpen />}</div>],
      ["Open, an option with keyboard focus", room(<Chip {...chip} initialOpen forceOption={{ id: "root", state: "focus" }} />)],
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
    // In place of the composer for an archived session (FLOW-ARCHIVE-DELETE-SESSION, step 2).
    name: "ArchivedNote",
    wide: true,
    states: [
      ["At rest", <ArchivedNote onResume={() => {}} />],
      ["Resume under the pointer", <ArchivedNote onResume={() => {}} force="hover" />],
      ["Resume pressed", <ArchivedNote onResume={() => {}} force="pressed" />],
      ["Resume, keyboard focus", <ArchivedNote onResume={() => {}} force="focus" />],
      ["Resuming", <ArchivedNote onResume={() => {}} resuming={{ state: "sending" }} />],
      ["Resuming, keyboard focus", <ArchivedNote onResume={() => {}} resuming={{ state: "sending" }} force="focus" />],
      [
        "Resume refused",
        <ArchivedNote onResume={() => {}} resuming={{ state: "failed", message: "session.resume: resuming a stopped session arrives with milestone M1" }} />,
      ],
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
      // From the moment of sending until the turn ends, send is stop.
      ["Sending", <Composer autoFocus={false} onSend={async () => false} initialText="Run the tests again." sending={{ state: "sending" }} onStop={() => {}} />],
      ["The agent working", <Composer autoFocus={false} onSend={async () => false} initialText="Then update the changelog." working onStop={() => {}} />],
      ["Stop under the pointer", <Composer autoFocus={false} onSend={async () => false} working onStop={() => {}} forceSend="hover" />],
      ["Stop pressed", <Composer autoFocus={false} onSend={async () => false} working onStop={() => {}} forceSend="pressed" />],
      ["Stop with keyboard focus", <Composer autoFocus={false} onSend={async () => false} working onStop={() => {}} forceSend="focus" />],
      // Stop turns back into send with focus kept, often with nothing left to send.
      ["Send with keyboard focus, nothing to send", <Composer autoFocus={false} onSend={async () => false} forceSend="focus" />],
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
      ["The attach menu open", menuRoom(<Composer {...attachProps} attachMenuOpen />)],
      ["An attach choice under the pointer", menuRoom(<Composer {...attachProps} attachMenuOpen forceAttachChoice={{ what: "images", state: "hover" }} />)],
      ["An attach choice pressed", menuRoom(<Composer {...attachProps} attachMenuOpen forceAttachChoice={{ what: "images", state: "pressed" }} />)],
      ["An attach choice with keyboard focus", menuRoom(<Composer {...attachProps} attachMenuOpen forceAttachChoice={{ what: "files", state: "focus" }} />)],
      ["Attachments, one that will be copied", <Composer {...attachProps} initialText="Match the mockup." initialAttachments={galleryAttachments} />],
      [
        "An attachment Ersa can't send",
        <Composer
          {...attachProps}
          initialText="Match the mockup."
          initialAttachments={[galleryAttachments[0], { ...galleryAttachments[1], problem: "This image is larger than Claude Code takes." }]}
        />,
      ],
      [
        "Copying attachments",
        statusRoom(<Composer {...attachProps} initialAttachments={galleryAttachments} sending={{ state: "sending", progress: { phase: "copying", share: 0.4 } }} onStop={() => {}} />),
      ],
      ["Starting the backend", statusRoom(<Composer {...attachProps} initialText="Match the mockup." sending={{ state: "sending", progress: { phase: "starting" } }} onStop={() => {}} />)],
      ["Can't tell whether it was sent", statusRoom(<Composer {...attachProps} initialText="Match the mockup." initialAttachments={galleryAttachments} sending={{ state: "unknown" }} />)],
      // FLOW-SEND-MESSAGE-R08: files dragged over the box, and an image pasted into the words.
      ["Files dragged over the box", <Composer {...attachProps} forceDragOver />],
      ["Files dragged over the box, focused", <Composer {...attachProps} forceDragOver force="focus" />],
      [
        "A pasted image",
        <Composer
          {...attachProps}
          initialText="This is the layout I mean."
          initialAttachments={[{ path: "pasted:1", name: "Pasted image", kind: "image", detail: "84 KB", inline: { media_type: "image/png", data: "" } }]}
        />,
      ],
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
