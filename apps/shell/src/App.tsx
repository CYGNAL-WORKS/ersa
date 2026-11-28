import { type Dispatch, type SetStateAction, useCallback, useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";
import { t } from "./catalog";
import "./App.css";
import { Button } from "./components/Button";
import type { AttachWhat } from "./components/AttachMenu";
import { ArchivedNote } from "./components/ArchivedNote";
import { Composer } from "./components/Composer";
import { CleanUpQuestion } from "./components/CleanUpQuestion";
import { DeleteQuestion } from "./components/DeleteQuestion";
import { DeveloperPane } from "./components/DeveloperPane";
import { RailFoot } from "./components/RailFoot";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { type Adding, type FolderChoice, NewSessionScreen } from "./components/NewSessionScreen";
import { QuitQuestion } from "./components/QuitQuestion";
import { SessionControls } from "./components/SessionControls";
import { SessionsPage } from "./components/SessionsPage";
import { SettingsPage } from "./components/SettingsPage";
import { SignInNote } from "./components/SignInNote";
import { unseenWrites } from "./session/unseen";
import { RailTop } from "./components/RailTop";
import { RailTree } from "./components/RailTree";
import { besideIn } from "./components/useFocusRestore";
import { Shelf } from "./components/Shelf";
import { Transcript } from "./components/Transcript";
import { Workbench } from "./components/Workbench";
import { type CoreClient, describeError } from "./core/client";
import { type Connection, type CoreView, type ProjectsView, type SessionsView, connect } from "./core/connection";
import { type QuitAsk, type ShellControl, answerQuit, busySessions, cancelQuit } from "./core/shell";
import type { BackendInfo, BackendSignInEndedNotification, Project, Session, SessionArrangeParams, Workspace } from "./protocol/core-api.gen";
import { type SessionValues, newestFirst, projectOrder, sessionShown } from "./session/controls";
import { backendLabel, contextUsed, tileOf } from "./session/tile";
import { type Answers, type Decision, type Reply, answering } from "./session/answers";
import { type ShelfShown, shelfAfter, shelfFollowing, shelfOf, shelfTone, shownAfter } from "./session/shelf";
import { sharedSubscriptions } from "./session/subscriptions";
import { type DeletePhase, Deletion } from "./session/deletion";
import { type Attached, attached, sendable, unreadable, waitingName } from "./session/attachments";
import { type Follower, type SendProgress, type SessionView, type Sending, type WaitingHere, follow, progressOf, waitingOf } from "./session/follow";
import { type Moving, sectionsUnavailable, useSections } from "./session/sections";
import { initialStage, showsScreen, stage } from "./session/stage";
import { RESET_PATCH, choicePatch, collapsePatch, useLocalDay, useRailArrangement } from "./session/arrangement";
import { noteState, sessionNoteFailure, signInKey, signedInClears, useNoteClears, useSignIns } from "./session/signIn";
import { StartStop, lastUsedFrom, readBackends, rememberPatch, startSession } from "./session/start";
import { mergeWorkspaces } from "./session/workspaces";
import { useWorkbench } from "./session/workbench";
import { useArchiveSetting } from "./session/archiveInactive";
import { useCleanUp } from "./session/cleanupList";
import { type RawLogReader, type RawLogView, readRawLog } from "./session/rawLog";
import { type Changeable, type Changes, NO_CHANGES, changeAnswered, changeSent, changeSession, isChangeable, refusalSeen, runs } from "./session/change";

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

/** The list a deletion was asked from: the sessions page's rows, or the rail's tiles. */
const deletionList = (fromPage: boolean | undefined) => document.querySelector(fromPage ? ".sessions-page" : ".rail-tree");

/** A number that changes whenever `value` is a new object: a key that resets an error boundary on new data. */
function useVersion(value: unknown): string {
  const seen = useRef({ value, version: 0 });
  if (seen.current.value !== value) seen.current = { value, version: seen.current.version + 1 };
  return String(seen.current.version);
}

/**
 * What a host's readings of its backends left: the last listing Core answered with, and why the latest failed, with
 * the connection it failed on.
 */
interface HostReading {
  listing?: BackendInfo[];
  failed?: { reason: string; link: unknown };
}

/** Whether a host's latest reading failed on `link`, the connection now: only its failures count. */
const failedOn = (r: HostReading, link: unknown) => r.failed !== undefined && r.failed.link === link;

export function App({ client, shell }: { client: CoreClient; shell?: ShellControl }) {
  const [view, setView] = useState<CoreView>({ phase: "connecting" });
  // The labeled endpoint a debug build is attached to, kept once Core has named it: the Shell fixes it at start, so it
  // holds while the connection is lost too, and Open diagnostic log stays unavailable (FLOW-READ-RAW-LOG-R13).
  const [attachedTo, setAttachedTo] = useState<string>();
  if (view.phase === "connected" && view.label !== undefined && view.label !== attachedTo) setAttachedTo(view.label);
  // Bumped to attach again; the effect below reconnects on each change.
  const [attempt, setAttempt] = useState(0);
  const retry = useCallback(() => setAttempt((n) => n + 1), []);
  const connection = useRef<Connection | null>(null);

  // What the stage shows: the active session, the new-session screen or the sessions page (session/stage.ts); and what the window
  // knows of the active session.
  const [{ activeId, composing, starting, firstSend, page }, dispatch] = useReducer(stage, initialStage);
  // A page stands over the stage: what is behind it is hidden and out of reach (FLOW-ORGANIZE-SESSIONS-R20; LAY-SHELL, The settings page).
  const onPage = page !== undefined;
  const [session, setSession] = useState<SessionView>();
  const follower = useRef<Follower | null>(null);
  // What this window sent that still waits in the backend, by session, kept while another session is shown, so the
  // status line names it again on coming back (FLOW-SEND-MESSAGE-R19). In memory only: a reload names none.
  const waitingKept = useRef<Record<string, readonly WaitingHere[] | undefined>>({});

  // What each host's readings of its backends left, by host id: the last listing Core answered with, and why the
  // latest reading failed, if it did. Held together so that one update settles both.
  const [hostReadings, setHostReadings] = useState<Record<string, HostReading>>({});
  const [lastUsed, setLastUsed] = useState<Record<string, SessionValues>>({});
  // Every project's workspaces, by id: the rail puts a session under its workspace's project, and the session
  // controls read a session's workspace from them. Each project's last read: null once read, else why it failed.
  const [workspaces, setWorkspaces] = useState<Record<string, Workspace>>({});
  const [workspaceReads, setWorkspaceReads] = useState<Record<string, string | null>>({});
  const [trusting, setTrusting] = useState<Record<string, Sending>>({});
  // The projects whose trust the user withdrew in this window, by id. Kept for the life of the Interface, which
  // closing the window hides rather than ends (FLOW-CLOSE-AND-QUIT-R01), and never saved.
  const [withdrawn, setWithdrawn] = useState<ReadonlySet<string>>(new Set());
  const [adding, setAdding] = useState<Adding>();
  // The quit question, while it is open: how many sessions would stop, and whether Quit was chosen.
  const [quitAsk, setQuitAsk] = useState<QuitAsk & { quitting: boolean; failed?: string }>();
  const [renaming, setRenaming] = useState<Record<string, Sending>>({});
  // Archives and unarchives on their way from a tile, and resumes from the archived note, by session id.
  const [archiving, setArchiving] = useState<Record<string, Sending>>({});
  const [resuming, setResuming] = useState<Record<string, Sending>>({});
  // Forks on their way from a tile or row, and why the last was refused, by the original's id.
  const [forking, setForking] = useState<Record<string, Sending>>({});
  // Core's settings, written as a merge patch: the rail's arrangement, the workbench and Storage's settings each send
  // theirs here.
  const saveSettings = useCallback((patch: object) => client.call("settings.set", { patch }), [client]);
  // How the rail is arranged (FLOW-ORGANIZE-SESSIONS): kept in Core's settings and read on each connection.
  const {
    arrangement,
    reading: arranging,
    problem: arrangeProblem,
    change: arrange,
    read: readArrangement,
    readFailed: arrangementUnread,
  } = useRailArrangement(saveSettings, describeError);
  // The workbench, open or closed and its width, kept in Core's settings for the window (FLOW-READ-RAW-LOG-R02).
  const workbench = useWorkbench(saveSettings);
  const readWorkbench = workbench.read;
  // Storage's Archive inactive sessions (FLOW-CLEAN-UP-R15), read with the rest of the settings and sent as a change.
  const archiveSetting = useArchiveSetting(saveSettings, describeError);
  const { read: readArchive, unread: archiveUnread } = archiveSetting;
  // Storage's Clean up list (FLOW-CLEAN-UP, steps 5 to 7), read each time the settings page opens while Core is reached.
  const cleanUp = useCleanUp(client, describeError);
  // Today, moved on at local midnight, so the date groups move with the passing of days alone (R07).
  const today = useLocalDay();
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

  // Core's word that a sign-in ended, for the sign-ins below (FLOW-SIGN-IN-R09).
  const signInEnded = useRef<(ending: BackendSignInEndedNotification) => Promise<void>>(undefined);
  useEffect(() => {
    const running = connect(client, setView, undefined, (notification) => {
      // Where a send is (FLOW-SEND-MESSAGE-R16): the active session's, or the first message of a session starting.
      if (notification.method === "session.send_progress") {
        follower.current?.progress(notification.params);
        if (notification.params.session_id === startStop.current?.session) setStartProgress(progressOf(notification.params));
        // A Resume from the archived note, still unanswered, waits for the session's folder to be put back: its note
        // says so until the resume is answered (FLOW-CLEAN-UP-R21). A report with no Resume under way here is not its.
        const id = notification.params.session_id;
        const progress = progressOf(notification.params);
        if (progress?.phase === "puttingBack") {
          setResuming((r) => (r[id]?.state === "sending" ? { ...r, [id]: { state: "sending", progress } } : r));
        }
        return;
      }
      if (notification.method === "backend.sign_in_ended") {
        void signInEnded.current?.(notification.params);
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
      // Never an archived one, which the rail does not show.
      const newest = newestFirst(sessions.sessions.filter((s) => !s.archived))[0];
      if (newest) dispatch({ type: "showNewest", id: newest.id });
    }
  }, [activeId, sessions]);

  const shownId = composing ? undefined : activeId;
  // As it is when an archive answers, which may be after the user chose another session.
  const shownNow = useRef(shownId);
  shownNow.current = shownId;

  // A change to a session's arrangement (FLOW-ORGANIZE-SESSIONS): Core stores it and answers with the session as
  // listed, and the rail shows the fields changed once Core has them. A refusal leaves the session as Core has it and
  // says why beneath its tile until the next try (R15). `track` holds the changes on their way and the refusals, each
  // with `tag`, such as which change it was. Resolves whether Core took it.
  type Arrange = Omit<SessionArrangeParams, "id">;
  const arrangeSession = async <T extends object>(track: Dispatch<SetStateAction<Record<string, Sending & T>>>, id: string, fields: Arrange, tag: T): Promise<boolean> => {
    track((t) => ({ ...t, [id]: { state: "sending", ...tag } }));
    try {
      const arranged = await client.call("session.arrange", { id, ...fields });
      track(({ [id]: _, ...rest }) => rest);
      // In no section, Core lists the session with no `section_id`, so the field is taken out.
      const keys = Object.keys(fields) as (keyof Arrange & keyof Session)[];
      connection.current?.changed(id, Object.fromEntries(keys.map((k) => [k, arranged[k]])));
      return true;
    } catch (error) {
      track((t) => ({ ...t, [id]: { state: "failed", message: describeError(error), ...tag } }));
      return false;
    }
  };

  // The unseen mark (FLOW-ORGANIZE-SESSIONS, Unseen), which Core stores so it survives a restart of Ersa and of Core
  // (R25). Ersa's own mark and Mark as unread or read (R24) both go through `markUnseen`. While one is on its way Ersa
  // sends no other for that session, so a list read before Core answered can't send it twice. One Core refused is
  // not sent again from the lists that follow, so its note stays (R15), until a new occasion for it: the user opens or
  // leaves a session, the session works again, Core is reached again, or the user chooses it from the menu.
  const [marking, setMarking] = useState<Record<string, Sending>>({});
  // The sessions with a mark on its way now, which a list may be reported before a render shows.
  const markSending = useRef(new Set<string>());
  // The sessions whose last mark Core refused.
  const markRefused = useRef(new Set<string>());
  const markUnseen = async (id: string, unseen: boolean) => {
    markSending.current.add(id);
    const taken = await arrangeSession(setMarking, id, { unseen }, {});
    markSending.current.delete(id);
    if (taken) markRefused.current.delete(id);
    else markRefused.current.add(id);
  };
  // The sessions working in the last list, to tell "idle after working" from plain idle. In memory: a turn that
  // finishes while Ersa is closed leaves no mark. Kept while Ersa can't reach Core, so a turn that ends meanwhile marks
  // its session from the first list once connected again (FLOW-ORGANIZE-SESSIONS-R38).
  const working = useRef<ReadonlySet<string>>(new Set());
  const lastShown = useRef(shownId);
  useEffect(() => {
    if (sessions.state !== "ready") {
      // Reaching Core again is a new occasion for a refused mark; what was working is kept (R38).
      markRefused.current.clear();
      return;
    }
    if (lastShown.current !== shownId) {
      for (const id of [lastShown.current, shownId]) if (id !== undefined) markRefused.current.delete(id);
      lastShown.current = shownId;
    }
    const next = unseenWrites(working.current, sessions.sessions, shownId, markSending.current, markRefused.current);
    // Only the step into working is a new occasion: a session that keeps working is asked nothing again.
    for (const id of next.working) if (!working.current.has(id)) markRefused.current.delete(id);
    working.current = next.working;
    for (const write of next.writes) void markUnseen(write.id, write.unseen);
    // markUnseen reads only refs and stable values.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessions, shownId]);

  useEffect(() => {
    if (activeId === undefined) return;
    const following = follow(
      shared(),
      activeId,
      (view) => {
        waitingKept.current[view.id] = view.waitingHere;
        setSession(view);
      },
      waitingKept.current[activeId],
    );
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
  // The user's sections, read on each connection; deleting one moves its sessions, so the list is read again (R05).
  const sections = useSections(client, link, describeError, () => connection.current?.reload("sessions"));
  useEffect(() => {
    if (link !== undefined) follower.current?.attached();
    else follower.current?.detached();
  }, [link, activeId]);
  // The active session's raw log for the developer pane (FLOW-READ-RAW-LOG), read while the workbench is open. One
  // reader for each client, made in an effect so React's second run in development makes a fresh one; the next effect
  // tells it what the window wants, after this one has made it.
  const [rawLog, setRawLog] = useState<RawLogView>();
  const rawLogReader = useRef<RawLogReader | undefined>(undefined);
  useEffect(() => {
    const reader = readRawLog(client, setRawLog);
    rawLogReader.current = reader;
    return () => {
      reader.stop();
      rawLogReader.current = undefined;
    };
  }, [client]);
  useEffect(() => rawLogReader.current?.show({ open: workbench.open, session: shownId, link }), [client, workbench.open, shownId, link]);
  // An answer Core took whose decision never came may be lost with the old connection (FLOW-APPROVE-PERMISSION-R32).
  useEffect(() => {
    if (link !== undefined) sending.attached();
  }, [sending, link]);

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

  // A host's backends, read on each new connection and again each time the new-session screen asks
  // (FLOW-NEW-SESSION-R50): Core may have found a backend since. A reading already out for the host answers for a
  // second ask, as when the window's showing and its focus arrive together. A fresh reading never answers from one
  // already out: a sign-in's step must see what Core has done since (FLOW-SIGN-IN-R09), which a reading that left
  // before it may not, so it follows that one. A reading from a connection since gone is dropped, and holds back none
  // on the new one. Each resolves with the listing it read, or none when it failed or was dropped.
  type Out = Promise<BackendInfo[] | undefined>;
  const readings = useRef<{ link: unknown; out: Map<string, Out>; next: Map<string, Out> }>({ link: undefined, out: new Map(), next: new Map() });
  const onListed = useRef<(hostId: string, listing: readonly BackendInfo[]) => void>(undefined);
  const readHost = useCallback(
    (hostId: string, fresh = false): Out => {
      if (link === undefined) return Promise.resolve(undefined);
      if (readings.current.link !== link) readings.current = { link, out: new Map(), next: new Map() };
      const r = readings.current;
      const already = r.out.get(hostId);
      if (already) {
        if (!fresh) return already;
        // One reading follows the one out, for every fresh ask made meanwhile.
        let next = r.next.get(hostId);
        if (!next) {
          next = already.then(() => {
            r.next.delete(hostId);
            return readings.current === r ? readHost(hostId) : undefined;
          });
          r.next.set(hostId, next);
        }
        return next;
      }
      const reading = readBackends(client, hostId).then((read) => {
        if (readings.current !== r) return undefined;
        r.out.delete(hostId);
        // Until the answer, the listing held stays, so a later reading shows nothing of itself (R50). A failure
        // keeps the last listing Core answered with behind it, and names the connection it failed on (below).
        setHostReadings((all) => ({
          ...all,
          [hostId]:
            read.failed === undefined
              ? { listing: read.backends }
              : { listing: all[hostId]?.listing, failed: { reason: read.failed, link: r.link } },
        }));
        if (read.failed !== undefined) return undefined;
        onListed.current?.(hostId, read.backends);
        return read.backends;
      });
      r.out.set(hostId, reading);
      return reading;
    },
    [client, link],
  );
  const readHostBackends = useCallback((hostId: string) => void readHost(hostId), [readHost]);
  // Sign in and Cancel on the sign-in note, and Core's word that a sign-in ended (FLOW-SIGN-IN).
  const readFresh = useCallback((hostId: string) => readHost(hostId, true), [readHost]);
  const signIns = useSignIns(client, shell?.openLink, readFresh);
  // The sign-in note under the shown session (FLOW-SIGN-IN-R17): where this window has cleared each session's note,
  // the backend the shown session's note is for, by `signInKey`, and the shown session's latest event, set as the
  // window renders below.
  const noteClears = useNoteClears();
  const sessionNoteKey = useRef<string>(undefined);
  const shownSeq = useRef<{ id: string; lastSeq: number }>(undefined);
  onListed.current = (hostId, listing) => signIns.listed(hostId, listing, sessionNoteKey.current);
  signInEnded.current = (ending) => {
    // Signed in: every session of that backend on that host loses its note, until a failure after this one (R17), once
    // the host's backends read on this end no longer report it signed out; until that reading answers, every note stays
    // as it was, and while it says signed out the notes stay, the shown one saying so (R12). A reading that fails clears
    // as one that says it can run. The marks are taken as the sign-in ends: the shown session through what it held then;
    // the others through their latest event as Core lists it now, since the window's list only learns a session's
    // latest event when it is read, and one out of view may have failed since.
    const reading = signIns.ended(ending);
    if (ending.outcome !== "signed_in") return reading.then(() => {});
    const shownThen = shownSeq.current;
    const atEnd = signedInClears(ending.host_id, ending.name, knownSessions.current ?? [], shownThen, false);
    const listedAfter = client.call("session.list").then(
      (listing) => signedInClears(ending.host_id, ending.name, listing, shownThen, true),
      () => undefined,
    );
    return reading.then(async (listing) => {
      if (listing?.find((b) => b.name === ending.name)?.status === "signed_out") return;
      noteClears.clear(atEnd);
      const after = await listedAfter;
      if (after) noteClears.clear(after);
    });
  };
  // Only this connection's failures count. A connection lost fails every reading still out on it, and the Shell
  // reports that failure and the loss by separate paths, in either order, so the failure may land before the window
  // knows the connection went. Counted by the connection it failed on, it says nothing from the new connection's
  // first frame: a host that holds a listing shows it until this connection's reading answers (R50), and one Core
  // never listed is detected again (R23). The answers still out from the last connection are dropped above.
  // What the screen offers: each host's backends, absent while none has been read. A host whose latest reading
  // failed offers none, and the screen says why (FLOW-NEW-SESSION-R11), even when it held a listing before.
  const backends = useMemo(
    () =>
      Object.fromEntries(
        Object.entries(hostReadings).flatMap(([host, r]) => (failedOn(r, link) ? [[host, []]] : r.listing ? [[host, r.listing]] : [])),
      ) as Record<string, BackendInfo[]>,
    [hostReadings, link],
  );
  // Why each host's backends could not be listed, by host id: the screen says detection failed (FLOW-NEW-SESSION-R11).
  const backendFailures = useMemo(
    () => Object.fromEntries(Object.entries(hostReadings).flatMap(([host, r]) => (r.failed && failedOn(r, link) ? [[host, r.failed.reason]] : []))),
    [hostReadings, link],
  );
  const hostId = view.phase === "connected" ? view.hello.host.id : undefined;
  useEffect(() => {
    if (hostId !== undefined) readHostBackends(hostId);
  }, [readHostBackends, hostId]);
  useEffect(() => {
    if (link === undefined) return;
    let live = true;
    // Read once and shared: the new-session screen's remembered values, and the rail's arrangement (R13).
    // Remembered values that cannot be read leave the screen at the defaults.
    client.call("settings.get").then(
      (read) => {
        if (!live) return;
        readArrangement(read);
        readWorkbench(read);
        readArchive(read);
        setLastUsed(lastUsedFrom(read));
      },
      (error) => {
        if (!live) return;
        arrangementUnread(describeError(error));
        archiveUnread(error);
        setLastUsed({});
      },
    );
    return () => {
      live = false;
    };
  }, [client, link, readArrangement, arrangementUnread, readWorkbench, readArchive, archiveUnread]);

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
  // The unknown workspaces the last finished read was for: until the read for the current ones answers, they may yet
  // be placed.
  const [unknownRead, setUnknownRead] = useState("");
  useEffect(() => {
    // Without a connection nothing has been read on the next one, so its sessions wait to be placed again.
    if (!projectList) {
      setWorkspaceReads({});
      return;
    }
    let live = true;
    const reads: Promise<unknown>[] = [];
    // Each project's read is applied as it answers, so one slow host holds back only its own tiles; a session that
    // fits no project waits for every read, which the Shell gives up on after its request time limit. Merged
    // into what is known (session/workspaces.ts): a read that answers late never drops a workspace.
    for (const p of projectList) {
      const answered = client.call("workspace.list", { project_id: p.id }).then(
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
      reads.push(answered);
    }
    void Promise.all(reads).then(() => {
      if (live) setUnknownRead(unknownWorkspaces);
    });
    return () => {
      live = false;
    };
    // A change to unknownWorkspaces is a reason to read again.
  }, [client, projectList, unknownWorkspaces]);
  const projectsVersion = useVersion(projects);
  const sessionsVersion = useVersion(sessions);
  const coreVersion = useVersion(view);
  const retrySubscribe = useCallback(() => follower.current?.attached(), []);

  // Stable for each session, so the transcript's memoised blocks do not re-render for it.
  const decide = useCallback((id: string, requestId: string, decision: Decision) => sending.decide(id, requestId, decision), [sending]);
  const decideShown = useMemo(() => (requestId: string, decision: Decision) => shownId !== undefined && decide(shownId, requestId, decision), [decide, shownId]);
  const answer = useCallback((id: string, requestId: string, reply: Reply) => sending.answer(id, requestId, reply), [sending]);
  const answerShown = useMemo(() => (requestId: string, reply: Reply) => shownId !== undefined && answer(shownId, requestId, reply), [answer, shownId]);

  // New session shows the screen; leaving it, by choosing a session, leaves nothing behind (FLOW-NEW-SESSION-R03).
  const newSession = (project: Project) => dispatch({ type: "newSession", projectId: project.id });
  // A group heading collapsed or expanded: kept in Core for the grouping in use alone (FLOW-ORGANIZE-SESSIONS-R12).
  const collapsed = new Set(arrangement.collapsed[arrangement.groupBy]);
  const toggleGroup = (key: string) => {
    const next = new Set(collapsed);
    if (!next.delete(key)) next.add(key);
    arrange(collapsePatch(arrangement.groupBy, [...next]));
  };
  const select = (id: string) => dispatch({ type: "select", id });

  // The first message starts the session; on a failure the screen says why and the composer keeps the message
  // (FLOW-NEW-SESSION-R08). Once started, it is the active session (R12).
  // Stop on the new-session screen reaches the start under way (FLOW-SEND-MESSAGE-R03).
  const startStop = useRef<StartStop>(undefined);
  const [startProgress, setStartProgress] = useState<SendProgress>();
  const showing = useRef(arrangement.show);
  showing.current = arrangement.show;
  const start = async (text: string, values: SessionValues, attachments: readonly Attached[]) => {
    dispatch({ type: "sending" });
    setStartProgress(undefined);
    const stop = new StartStop(client);
    startStop.current = stop;
    // Known before the session is, so the rail puts the new session's tile under its project as it appears (FLOW-NEW-SESSION-R13).
    const known = (workspace: Workspace) => setWorkspaces((was) => ({ ...was, [workspace.id]: workspace }));
    const outcome = await startSession(client, values, text, stop, sendable(attachments), known).finally(() => {
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
    // Showing archived sessions only, the rail switches to active ones, so the new tile is in view (FLOW-ORGANIZE-SESSIONS-R21).
    // As it is now, not as it was when the start began: the user can change it while the session starts.
    if (showing.current === "archived") arrange(choicePatch({ show: "active" }));
    setLastUsed((l) => ({ ...l, [values.project_id]: values }));
    // Best effort: a failure only means the screen starts at these values in this window alone.
    client.call("settings.set", { patch: rememberPatch(values) }).catch(() => {});
    connection.current?.reload("sessions");
    dispatch({ type: "started", id: outcome.session.id, text, attachments, firstSend: outcome.firstSend });
    return true;
  };

  // The backend keeps the title (PROTO-CORE-API-R12); Core's answer shows it, in the rail and here, with no list read.
  const rename = async (id: string, title: string) => {
    setRenaming((r) => ({ ...r, [id]: { state: "sending" } }));
    try {
      // Core answers with the title as the backend will list it.
      const renamed = await client.call("session.set_title", { id, title });
      setRenaming(({ [id]: _, ...rest }) => rest);
      connection.current?.changed(id, { title: renamed.title ?? title });
      return true;
    } catch (error) {
      setRenaming((r) => ({ ...r, [id]: { state: "failed", message: describeError(error) } }));
      return false;
    }
  };

  // A running session's model, effort or permission mode, changed from its session controls (FLOW-NEW-SESSION-R30):
  // the chip reads the old value until Core answers with the backend's word on the new one; a refusal keeps it and
  // the status line says why until the user sends or opens a chip.
  const [changing, setChanging] = useState<Record<string, Changes>>({});
  const changesOf = (id: string, update: (c: Changes) => Changes) => setChanging((all) => ({ ...all, [id]: update(all[id] ?? NO_CHANGES) }));
  const change = async (id: string, key: Changeable, value: string) => {
    changesOf(id, (c) => changeSent(c, key));
    try {
      const changed = await changeSession(client, id, key, value);
      connection.current?.changed(id, changed);
      changesOf(id, (c) => changeAnswered(c, key));
    } catch (error) {
      changesOf(id, (c) => changeAnswered(c, key, (error as Error).message));
    }
  };
  const resetChange = (id: string) => changesOf(id, refusalSeen);

  // Pin, Unpin and Move to section on a tile (FLOW-ORGANIZE-SESSIONS-R01, R03): Core stores the pin and the section,
  // and the rail moves the tile once Core has them. A refusal leaves the tile where Core has it and says why beneath
  // it until the next try (R15).
  const [moving, setMoving] = useState<Record<string, Moving>>({});
  const pin = (id: string, pinned: boolean) => void arrangeSession<Pick<Moving, "change">>(setMoving, id, { pinned }, { change: "pin" });
  // Moving a pinned session to a section unpins it in the same change, so the move shows; to no section, the pin
  // stays (R22). A move into a section being made reads the pin as listed when its name was entered: the session's
  // Pin and Unpin wait while Core makes it (`movingShown`), so the pin can't change in this window meanwhile.
  const moveToSection = (id: string, sectionId: string | null) => {
    const pinned = sessions.state === "ready" && sessions.sessions.some((s) => s.id === id && s.pinned);
    const fields = sectionId !== null && pinned ? { section_id: sectionId, pinned: false } : { section_id: sectionId };
    void arrangeSession<Pick<Moving, "change">>(setMoving, id, fields, { change: "section" });
  };
  // A new section's heading, while it is shown with its name field: from the arrange menu, or from a session's Move
  // to section, which moves that session into it once named (FLOW-ORGANIZE-SESSIONS, steps 3 and 5). Sections are
  // seen only grouped by section, so choosing it under another grouping switches to grouping by section (R36).
  // `opener` is what the menu gave focus back to as New section was chosen (More, the tile, or the arrange button),
  // which takes it again when nothing is made.
  const [newSection, setNewSection] = useState<{ for?: string; opener?: HTMLElement }>();
  const startNewSection = (forSession?: string) => {
    sections.createReset();
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
    setNewSection({ for: forSession, opener });
    if (arrangement.groupBy !== "section") arrange(choicePatch({ groupBy: "section" }));
  };
  // A section being made for a session counts as that session's change on its way, so Pin, Unpin and Move to section
  // wait for it, one at a time, and its move is measured from where it is (R22).
  const movingShown: Record<string, Moving> =
    newSection?.for !== undefined && sections.creating?.state === "sending" ? { ...moving, [newSection.for]: { state: "sending", change: "section" } } : moving;
  // Grouped by anything else, as when Core refused the switch, the heading has nowhere to show, so it goes.
  if (newSection && arrangement.groupBy !== "section") setNewSection(undefined);
  const nameNewSection = async (name: string) => {
    const section = await sections.create(name);
    if (!section) return undefined;
    const forSession = newSection?.for;
    setNewSection(undefined);
    if (forSession !== undefined) moveToSection(forSession, section.id);
    return section;
  };

  // Archive and Unarchive from a tile (FLOW-ARCHIVE-DELETE-SESSION, steps 1 and 3): Core stops a live session as it
  // archives it, and keeps its pin and section either way. The list is read again to show the change; archiving the
  // active session shows the new-session screen, since its tile has left the rail; behind the sessions page, if it shows.
  const archive = async (id: string, archived: boolean) => {
    setArchiving((a) => ({ ...a, [id]: { state: "sending" } }));
    try {
      await client.call("session.set_archived", { id, archived });
      setArchiving(({ [id]: _, ...rest }) => rest);
      if (archived && id === shownNow.current) dispatch({ type: "activeLeft" });
      connection.current?.reload("sessions");
    } catch (error) {
      setArchiving((a) => ({ ...a, [id]: { state: "failed", message: describeError(error) } }));
    }
  };

  // Fork from a tile or row (FLOW-FORK-SESSION): Core starts the fork and answers with it as listed; it becomes the
  // active session (R04), and its tile slides in at its place once the list is read again. A refusal, or a workspace
  // Core can't make, leaves the original as it was and says why beneath its tile until the next try.
  const fork = async (id: string) => {
    setForking((f) => ({ ...f, [id]: { state: "sending" } }));
    try {
      const forked = await client.call("session.fork", { id });
      setForking(({ [id]: _, ...rest }) => rest);
      // A fork is never archived: showing archived sessions only, the rail switches to active ones, so its tile is
      // in view, as after a new session's start (FLOW-ORGANIZE-SESSIONS-R21).
      if (showing.current === "archived") arrange(choicePatch({ show: "active" }));
      connection.current?.reload("sessions");
      dispatch({ type: "select", id: forked.id });
    } catch (error) {
      setForking((f) => ({ ...f, [id]: { state: "failed", message: describeError(error) } }));
    }
  };

  // Resume from an archived session's note (step 2): Core clears the archived mark as it accepts the resume, and
  // leaves the session archived when it refuses, which the note then says (PROTO-CORE-API-R43).
  const resume = async (id: string) => {
    setResuming((r) => ({ ...r, [id]: { state: "sending" } }));
    try {
      await client.call("session.resume", { id });
      setResuming(({ [id]: _, ...rest }) => rest);
    } catch (error) {
      setResuming((r) => ({ ...r, [id]: { state: "failed", message: describeError(error) } }));
    }
    // Read again either way: an accepted resume unarchived the session, and a refused one may have changed nothing.
    connection.current?.reload("sessions");
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
            setAdding({ state: "finding" });
            const found = await client.call("project.find", { host_id, path });
            setAdding(undefined);
            return found ? { kind: "found", project: found } : { kind: "new", path };
          } catch (error) {
            setAdding({ state: "failed", message: describeError(error) });
            return { kind: "failed" };
          }
        }
      : undefined;

  // The delete question, while it is open (FLOW-ARCHIVE-DELETE-SESSION, steps 4 to 6): the session, as its tile
  // names it, where the question stands, and the round trips behind it.
  // `fromPage`: asked from a row on the sessions page, so focus comes back there rather than to the rail.
  const [deleteAsk, setDeleteAsk] = useState<{ id: string; title: string; phase: DeletePhase; run?: Deletion; fromPage?: boolean }>();
  // The tile focus goes to once the question closes, by its session's id; New session when none is left. Moved in an
  // effect, after the new-session screen a deleted active session leaves behind has taken focus as it opened.
  const [refocus, setRefocus] = useState<{ to?: string; fromPage?: boolean; composer?: boolean }>();
  // The active session as it is when a deletion ends, which may be after the user chose another.
  const activeNow = useRef(activeId);
  activeNow.current = activeId;
  useEffect(() => {
    if (!refocus) return;
    const item = refocus.to && deletionList(refocus.fromPage)?.querySelector(`[data-session-id="${CSS.escape(refocus.to)}"] .session`);
    const composer = refocus.composer ? document.querySelector<HTMLElement>("main:not([hidden]) .composer-words") : null;
    (composer ?? (item instanceof HTMLElement ? item : document.querySelector<HTMLElement>(refocus.fromPage ? ".sessions-page-new" : ".rail-new-session")))?.focus({ preventScroll: true });
    setRefocus(undefined);
  }, [refocus]);
  // The question as last drawn: a deletion that ends speaks only for the question still showing it.
  const askNow = useRef(deleteAsk);
  askNow.current = deleteAsk;
  const askDelete = (id: string, fromPage?: boolean) => {
    // One deletion at a time: while one runs, its question stays until it ends.
    if (askNow.current?.phase.kind === "deleting") return;
    const listed = sessions.state === "ready" ? sessions.sessions.find((s) => s.id === id) : undefined;
    if (!listed) return;
    const tile = tileOf(listed, backends);
    const workspace = workspaces[listed.workspace_id];
    const ask = { id, title: tile.title, fromPage };
    // The rail places a session only once its workspace is read, so this is rare: Try again asks once it is.
    if (!workspace) {
      setDeleteAsk({ ...ask, phase: { kind: "failed", reason: t("rail.delete.workspaceUnknownError") } });
      return;
    }
    const run = new Deletion(client, id, workspace, (phase) => {
      // A question closed or replaced since hears nothing more from its run, which has nothing left to delete.
      if (askNow.current?.run !== run) return;
      setDeleteAsk((was) => (was?.run === run ? { ...was, phase } : was));
      if (phase.kind === "deleted") deleted(id);
      // Core stopped the session before it refused or failed, so the rail reads it again (R07).
      else if (phase.kind === "failed" || (phase.kind === "asking" && phase.changed)) connection.current?.reload("sessions");
    });
    const asked = { ...ask, phase: { kind: "reading" } as DeletePhase, run };
    // Known at once: a root workspace's question is ready before the next render.
    askNow.current = asked;
    setDeleteAsk(asked);
    void run.start();
  };
  // Deleted: focus goes to the next tile in the rail, else the previous, else New session (docs/design/RAIL.md, The
  // delete question); the active session leaves the new-session screen in its place.
  const deleted = (id: string) => {
    // The same tile any leaving gives focus to, by the same rule (besideIn).
    const fromPage = askNow.current?.fromPage;
    const list = deletionList(fromPage);
    setDeleteAsk(undefined);
    setRefocus({ to: list ? besideIn(list, id)?.dataset.sessionId : undefined, fromPage });
    if (id === activeNow.current) dispatch({ type: "activeLeft" });
    connection.current?.reload("sessions");
  };
  // While the user's own deletion runs, its tile reads Deleting at once (LAY-SHELL, Being deleted): Core marks the
  // session as it starts deleting, and the rail reads the list again only once it ends.
  const deletingId = deleteAsk?.phase.kind === "deleting" ? deleteAsk.id : undefined;
  const railSessions = useMemo<SessionsView>(
    () =>
      deletingId && sessions.state === "ready"
        ? { ...sessions, sessions: sessions.sessions.map((s) => (s.id === deletingId ? { ...s, deleting: "running" as const } : s)) }
        : sessions,
    [sessions, deletingId],
  );
  // While the question is open the rest of the window is inert, so neither a click nor Tab takes focus out of it
  // (docs/design/RAIL.md, The delete question); it is not dimmed.
  // The list names items by their sessions' titles and their projects, as the window holds them.
  const cleanUpNames = useMemo(() => ({ sessions: sessions.state === "ready" ? sessions.sessions : [], projects: projectList ?? [] }), [sessions, projectList]);
  // Opening the settings page reads the list, and so does Core coming back while it shows (FLOW-CLEAN-UP, Trigger).
  const readCleanUp = useRef(cleanUp.read);
  readCleanUp.current = cleanUp.read;
  const reached = view.phase === "connected";
  useEffect(() => {
    if (page === "settings" && reached) readCleanUp.current();
  }, [page, reached]);
  // The clean-up question stands over the window in the same way (docs/design/SETTINGS.md, The clean-up question).
  const behind = (deleteAsk !== undefined && deleteAsk.phase.kind !== "deleted") || cleanUp.view.run !== undefined;
  // Cancel leaves everything as it was, and focus goes back to the tile.
  const cancelDelete = () => {
    const id = deleteAsk?.id;
    const fromPage = deleteAsk?.fromPage;
    setDeleteAsk(undefined);
    setRefocus({ to: id, fromPage });
  };

  // A refusal goes; a rename still on its way stays, so its answer is shown.
  const resetRename = (id: string) => setRenaming(({ [id]: was, ...rest }) => (was?.state === "sending" ? { ...rest, [id]: was } : rest));

  // Attaching from the menu (FLOW-SEND-MESSAGE-R07): the Shell's dialog on this computer, then `read` below for the
  // paths picked. A path Core can't read is attached marked with why, so the message waits until it is removed (R10).
  // Without the Shell, as in tests that give none, attach is unavailable.
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
          } catch (error) {
            // A dialog that failed to open is not a cancel: logged here, and the composer says so (FLOW-SEND-MESSAGE-R23).
            console.error(error);
            throw error;
          }
          return read(paths, workspace);
        }
      : undefined;

  // Every project the Interface adds is trusted: Trust is the trust question's one answer that adds (FLOW-NEW-SESSION-R25).
  const addProject = async (path: string) => {
    if (view.phase !== "connected") return undefined;
    setAdding({ state: "adding" });
    try {
      const project = await client.call("project.add", { host_id: view.hello.host.id, path, trusted: true });
      setAdding(undefined);
      return project;
    } catch (error) {
      setAdding({ state: "failed", message: describeError(error) });
      return undefined;
    } finally {
      // Read again either way: an add that timed out may have finished in Core all the same.
      connection.current?.reload("projects");
      // The folder's sessions started in the backend's own tools are listed now (FLOW-RESUME-SESSION-R12).
      connection.current?.reload("sessions");
    }
  };

  const setTrust = async (project: Project, trusted: boolean) => {
    setTrusting((s) => ({ ...s, [project.id]: { state: "sending" } }));
    try {
      await client.call("project.set_trust", { id: project.id, trusted });
      setTrusting(({ [project.id]: _, ...rest }) => rest);
      connection.current?.reload("projects");
      // Ersa's withdrawal leaves the backend's own record of the folder's trust (ADR 0028), so the heading says
      // so while the project stays untrusted (FLOW-NEW-SESSION-R40, R41).
      if (!trusted) setWithdrawn((w) => new Set(w).add(project.id));
    } catch (error) {
      setTrusting((s) => ({ ...s, [project.id]: { state: "failed", message: describeError(error) } }));
    }
  };

  // Said beneath a project whose trust the user withdrew here, while Core still lists it untrusted: trusted
  // again from anywhere, the note no longer applies.
  const backendTrusts = projects.state === "ready" ? projects.projects.filter((p) => !p.trusted && withdrawn.has(p.id)).map((p) => p.id) : [];
  // Why each project's workspaces could not be read; and whether some project's are still being read, in which
  // case a session the rail cannot place yet waits rather than sit at the end.
  const workspaceErrors: Record<string, string> = {};
  for (const [id, failure] of Object.entries(workspaceReads)) if (failure !== null) workspaceErrors[id] = failure;
  const placing = projectList !== undefined && !allRead;
  // The projects were read again for a workspace no read had listed, and that read has not answered.
  const reading = allRead && unknownWorkspaces !== unknownRead;

  // The title as last listed, so the header keeps it while the list reloads
  // or Core is away.
  const titles = useRef<Record<string, string>>({});
  if (sessions.state === "ready") for (const s of sessions.sessions) if (s.title) titles.current[s.id] = s.title;
  const shown = !composing && session && session.id === activeId ? session : undefined;
  // The active session's backend by its own label, as its tile names it, for the raw log's directions (FLOW-READ-RAW-LOG-R03).
  const rawLogSession = sessions.state === "ready" ? sessions.sessions.find((s) => s.id === shownId) : undefined;
  const rawLogBackend = rawLogSession ? backendLabel(rawLogSession, backends) : "";
  const firstSendHere = firstSend && firstSend.id === shown?.id ? firstSend : undefined;

  // A file dropped while the sessions page or the settings page shows reaches no composer behind it.
  const dragDrop = onPage ? undefined : shell?.onDragDrop;

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
  // a running one's model, effort and permission mode change through Core (R30). Absent until its workspace and project are known.
  const listed = sessions.state === "ready" ? sessions.sessions.find((s) => s.id === shown?.id) : undefined;
  // Ersa can't reach the sessions: the composer says so and is unavailable until it does (FLOW-SEND-MESSAGE-R06).
  const away = view.phase === "connected" ? undefined : view.phase === "failed" ? "unreachable" : "reconnecting";
  const listedWorkspace = listed && workspaces[listed.workspace_id];
  const listedProject = listedWorkspace && projectById.get(listedWorkspace.project_id);
  // While Ersa can't reach Core its lists are gone: the controls keep the session's values and project as last listed,
  // unavailable, rather than leaving the composer (FLOW-NEW-SESSION-R35).
  const keptControls = useRef<{ session: Session; project: Project }>(undefined);
  if (listed && listedProject) keptControls.current = { session: listed, project: listedProject };
  const controlsFor =
    listed && listedProject ? { session: listed, project: listedProject } : away && keptControls.current?.session.id === shown?.id ? keptControls.current : undefined;
  const controlsWorkspace = controlsFor && workspaces[controlsFor.session.workspace_id];
  const sessionControls = controlsFor && controlsWorkspace && sessionShown(controlsFor.session, controlsWorkspace, backends[controlsFor.session.host_id] ?? []);
  // Whether the shown session is archived, as last listed: after a reconnect, until the sessions are listed again,
  // an archived session keeps its note in place of the composer (FLOW-ARCHIVE-DELETE-SESSION-R02). While Ersa can't
  // reach Core the composer shows instead, saying so and unavailable (FLOW-SEND-MESSAGE-R06).
  const kept = keptControls.current?.session;
  const archived = listed ? listed.archived : !away && shown !== undefined && kept?.id === shown.id && kept.archived;
  const used = listed && contextUsed(listed.tile);
  const changeHere = shown ? (changing[shown.id] ?? NO_CHANGES) : NO_CHANGES;

  // The sign-in note under the shown session (FLOW-SIGN-IN-R17): while it is live, not archived, and its latest
  // failure as not signed in has no message of the person's after it and was not cleared since, for the session's
  // backend on its host. It follows the session's own failure, not the listing, which may still say available.
  const noteFailure =
    shown && listed && !listed.archived && runs(listed) ? sessionNoteFailure(shown.transcript.items, noteClears.clears[shown.id] ?? -1) : undefined;
  const noteKey = listed && noteFailure !== undefined ? signInKey(listed.host_id, listed.backend) : undefined;
  sessionNoteKey.current = noteKey;
  shownSeq.current = shown && { id: shown.id, lastSeq: shown.transcript.lastSeq };
  const sessionNote = listed && noteKey !== undefined && (
    <SignInNote
      label={backendLabel(listed, backends)}
      place="session"
      // The sign-in under way, as the host's listing reports it, whatever it says of the backend's status (R06).
      state={noteState({ signing_in: backends[listed.host_id]?.find((b) => b.name === listed.backend)?.signing_in }, signIns.signIns[noteKey])}
      onSignIn={() => void signIns.signIn(listed.host_id, listed.backend)}
      onCancel={() => void signIns.cancel(listed.host_id, listed.backend)}
    />
  );
  // A failure the shown session's note has not shown for before starts it afresh, so it says nothing of an earlier
  // sign-in (R15). Before the page is painted, so the earlier words never show with it.
  const noteShownFor = useRef(new Map<string, number>());
  const forgetSaid = signIns.forget;
  const noteSessionId = shown?.id;
  useLayoutEffect(() => {
    if (noteSessionId === undefined) return;
    // While the note is not shown, the next failure is new to it, whatever its number in whichever view.
    if (noteFailure === undefined || noteKey === undefined) return void noteShownFor.current.delete(noteSessionId);
    if (noteShownFor.current.get(noteSessionId) === noteFailure) return;
    noteShownFor.current.set(noteSessionId, noteFailure);
    forgetSaid(noteKey);
  }, [noteSessionId, noteFailure, noteKey, forgetSaid]);
  // A mark holds only while its session stays live on this connection (useNoteClears).
  const liveIds = sessions.state === "ready" ? sessions.sessions.filter((s) => runs(s)).map((s) => s.id).sort().join(" ") : undefined;
  const keepClears = noteClears.keep;
  useEffect(() => {
    if (!connected) keepClears(new Set());
    else if (liveIds !== undefined) keepClears(new Set(liveIds.split(" ").filter(Boolean)));
  }, [connected, liveIds, keepClears]);

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
  // Kept by the shelf the window holds open, not the one drawn: while Core is away nothing is drawn, and the same
  // shelf counts on from where it was once Core lists its sessions again (R09).
  const nextShown = shownAfter(shelfShown, shelfId, shelfShows);
  if (nextShown !== shelfShown) setShelfShown(nextShown);
  const shelfOpen = (() => {
    if (!shelfSession || !shelfShows) return undefined;
    const tile = tileOf(shelfSession, backends);
    return {
      id: shelfSession.id,
      tone: shelfTone(shelfShows, nextShown, shelfSession.tile.pending),
      render: (domId: string) => (
        <Shelf
          id={domId}
          tile={tile}
          shows={shelfShows}
          onRetry={shelf.attached}
          answers={answers[shelfSession.id]}
          live={connected}
          onDecide={(requestId, decision) => decide(shelfSession.id, requestId, decision)}
          onAnswer={(requestId, reply) => answer(shelfSession.id, requestId, reply)}
        />
      ),
    };
  })();

  const railWaits = arranging || (arrangement.groupBy === "section" && sections.view.state === "loading");
  return (
    <div className="app">
      <nav className="rail" aria-label="Rail" inert={behind}>
        {/* Unavailable until Ersa has connected (LAY-SHELL, The foot). */}
        <RailTop
          onNewSession={() => dispatch({ type: "newSession" })}
          disabled={view.phase !== "connected"}
          arrange={{
            arrangement,
            reading: arranging,
            // Changing what is shown never closes the active session, even when its tile leaves the rail.
            onGroupBy: (groupBy) => arrange(choicePatch({ groupBy })),
            onSortBy: (sortBy) => arrange(choicePatch({ sortBy })),
            onShow: (show) => arrange(choicePatch({ show })),
            onNewSection: () => startNewSection(),
            newSectionUnavailable: sectionsUnavailable(sections.view),
            onReset: () => arrange(RESET_PATCH),
            onAllSessions: () => dispatch({ type: "allSessions" }),
            workbenchOpen: workbench.open,
            onDeveloperPane: () => workbench.setOpen(!workbench.open),
            onSettings: () => dispatch({ type: "settings" }),
          }}
        />
        <div className="rail-body">
          <ErrorBoundary what={t("rail.tree.boundaryName")} resetKey={`${projectsVersion}:${sessionsVersion}`}>
            <RailTree
              // Grouped as the user arranged it once that is read, and grouped by section once the sections are, so
              // the tree never draws one way and then regroups.
              projects={railWaits && projects.state === "ready" ? { state: "loading" } : projects}
              groupBy={arrangement.groupBy}
              sortBy={arrangement.sortBy}
              show={arrangement.show}
              now={today}
              arrangeProblem={arrangeProblem}
              sessions={railWaits && railSessions.state === "ready" ? { state: "loading" } : railSessions}
              sectionHandlers={{
                onRename: sections.rename,
                renaming: sections.renaming,
                onRenameReset: sections.renameReset,
                onDelete: (id) => void sections.remove(id),
                removing: sections.removing,
              }}
              newSection={newSection && { ...newSection, creating: sections.creating, onName: nameNewSection, onReset: sections.createReset, onCancel: () => setNewSection(undefined) }}
              workspaces={workspaces}
              workspaceErrors={workspaceErrors}
              placing={placing}
              reading={reading}
              activeId={shownId}
              onSelect={select}
              onNewSession={newSession}
              collapsed={collapsed}
              onToggle={toggleGroup}
              onPin={pin}
              onMove={moveToSection}
              moving={movingShown}
              sections={sections.view}
              onNewSection={startNewSection}
              onMark={markUnseen}
              marking={marking}
              onRename={rename}
              renaming={renaming}
              onRenameReset={resetRename}
              onFork={fork}
              forking={forking}
              onArchive={archive}
              archiving={archiving}
              onDelete={askDelete}
              offline={view.phase !== "connected"}
              onSetTrust={setTrust}
              trusting={trusting}
              backendTrusts={backendTrusts}
              localHost={hello?.host.id}
              hostNames={hostNames}
              backends={backends}
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
            <RailFoot view={view} retried={attempt > 0} onRetry={retry} />
          </ErrorBoundary>
        </div>
      </nav>
      {/* Beside the rail: the stage, and the workbench at the right, which the stage narrows to make room for
          (LAY-SHELL, The workbench). The workbench is outside the stage, so the sessions page leaves it in place. */}
      <div className="work">
      <div className="stages">
      {page === "settings" && (
        // As the sessions page: the stage behind is hidden and out of reach, its session still active (LAY-SHELL, The
        // settings page).
        <main className="stage" inert={behind}>
          <ErrorBoundary what={t("settings.boundaryName")}>
            <SettingsPage
              archive={archiveSetting.view}
              changing={archiveSetting.changing}
              offline={view.phase !== "connected"}
              onArchive={archiveSetting.choose}
              cleanUp={cleanUp.view}
              onCleanUp={cleanUp.actions}
              names={cleanUpNames}
            />
          </ErrorBoundary>
        </main>
      )}
      {page === "sessions" && (
        // The page stands over the stage: the composer, the session controls and the header are hidden and out of reach,
        // so nothing typed reaches the active session, which stays active (FLOW-ORGANIZE-SESSIONS-R20).
        <main className="stage" inert={behind}>
          <ErrorBoundary what={t("rail.sessionsPage.boundaryName")} resetKey={sessionsVersion}>
            <SessionsPage
              sessions={railSessions}
              projects={projectList}
              workspaces={workspaces}
              localHost={hello?.host.id}
              hostNames={hostNames}
              backends={backends}
              onOpen={(id) => {
                select(id);
                // The row that had focus is gone. A composer kept behind the page doesn't take it again as one drawn
                // afresh does, and an archived session has none: its tile does.
                setRefocus({ to: id, composer: true });
              }}
              onNewSession={() => dispatch({ type: "newSession" })}
              onPin={pin}
              onMove={moveToSection}
              moving={movingShown}
              sections={sections.view}
              onNewSection={startNewSection}
              onMark={markUnseen}
              marking={marking}
              activeId={shownId}
              onRename={rename}
              renaming={renaming}
              onRenameReset={resetRename}
              onFork={fork}
              forking={forking}
              onArchive={archive}
              archiving={archiving}
              onDelete={(id) => askDelete(id, true)}
              offline={view.phase !== "connected"}
            />
          </ErrorBoundary>
        </main>
      )}
      {shown ? (
        <main className="stage" hidden={onPage} inert={behind || onPage}>
          {/* Tries again on each new event, and keeps the composer out of it. */}
          <ErrorBoundary what="This session" resetKey={`${shown.id}:${shown.transcript.lastSeq}`}>
            <Transcript
              key={shown.id}
              title={(activeId && titles.current[activeId]) || t("rail.tile.untitledLabel")}
              untitled={!(activeId && titles.current[activeId])}
              view={shown}
              answers={answers[shown.id]}
              onDecide={decideShown}
              onAnswer={answerShown}
              onRetry={retrySubscribe}
              onRename={(title) => rename(shown.id, title)}
              renaming={renaming[shown.id]}
              // A refusal goes; a rename still on its way stays, so its answer is shown.
              onRenameReset={() => resetRename(shown.id)}
              renameDisabled={view.phase !== "connected"}
              onOpenLink={shell?.openLink}
              workspace={listedWorkspace?.path}
              // By its label, as its tile names it: the line for a message that failed as not signed in names it (FLOW-SIGN-IN-R16).
              backend={listed && backendLabel(listed, backends)}
            />
          </ErrorBoundary>
          <ErrorBoundary what="The composer" resetKey={shown.id}>
            {archived ? (
              // An archived session opens read-only: a note with Resume in place of the composer (FLOW-ARCHIVE-DELETE-SESSION-R02).
              <ArchivedNote onResume={() => void resume(shown.id)} resuming={resuming[shown.id]} />
            ) : (
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
              dragDrop={dragDrop}
              workspace={listed?.workspace_id}
              onSend={(text, attachments) => {
                // Sending clears a refused change's line (FLOW-NEW-SESSION-R30).
                resetChange(shown.id);
                // A message of the person's clears the sign-in note once Core has it (FLOW-SIGN-IN-R17), through the
                // events known as it was sent, so a failure of this message that arrives before Core's answer still shows.
                const id = shown.id;
                const through = shown.transcript.lastSeq;
                const sent = follower.current?.send(text, sendable(attachments), waitingName(text, attachments)) ?? Promise.resolve(false);
                return sent.then((accepted) => {
                  if (accepted) noteClears.clear({ [id]: through });
                  return accepted;
                });
              }}
              note={sessionNote || undefined}
              refusedChange={changeHere.refused}
              // Stop, from sending until the turn ends: cancels the message on its way, or interrupts the turn (FLOW-SEND-MESSAGE-R03).
              // While it works, Enter still sends, and the message waits in the backend (R18).
              working={shown.transcript.working || shown.turnPending}
              onStop={() => void follower.current?.interrupt()}
              // Once chosen, stop takes no second choice until the turn ends, which would stop what waits (R20).
              stopping={shown.stopTaken}
              waiting={waitingOf(listed?.tile.waiting, shown.waitingHere)}
              controls={
                sessionControls &&
                controlsFor &&
                hello && (
                  <SessionControls
                  hosts={[hello.host]}
                  localHostId={hello.host.id}
                  projects={[controlsFor.project]}
                  backends={[sessionControls.backend]}
                  values={sessionControls.values}
                  onChoose={(key, id) => {
                    if (isChangeable(key)) void change(shown.id, key, id);
                  }}
                  onOpen={() => resetChange(shown.id)}
                  readOnly
                  changeable={runs(controlsFor.session)}
                  waiting={changeHere.sending}
                  // Nothing reaches Core while Ersa can't reach it.
                  disabled={away !== undefined}
                  used={used}
                  windows={shown.transcript.windows}
                  />
                )
              }
            />
            )}
          </ErrorBoundary>
        </main>
      ) : hello && screenProjects && showsScreen({ composing, starting, activeId }, view.phase === "connected") ? (
        <main className="stage" hidden={onPage} inert={behind || onPage}>
          <ErrorBoundary what="The new-session screen" resetKey={composing?.projectId ?? ""}>
            <NewSessionScreen
              // Drawn afresh on each New session, so its composer takes focus (FLOW-NEW-SESSION-R01).
              key={composing?.key ?? 0}
              hosts={[hello.host]}
              localHostId={hello.host.id}
              projects={screenProjects}
              backends={backends}
              backendFailures={backendFailures}
              // Behind either page the screen is not showing: coming back from it shows it again (R50).
              shown={!onPage}
              onReadBackends={readHostBackends}
              signIns={signIns.signIns}
              onSignIn={signIns.signIn}
              onCancelSignIn={signIns.cancel}
              onWindowBack={shell?.onWindowBack}
              lastUsed={lastUsed}
              projectId={composing?.projectId}
              onStart={start}
              onAttach={attach}
              onRead={read}
              dragDrop={dragDrop}
              rootWorkspaces={rootWorkspaces}
              starting={starting?.state === "sending" ? { state: "sending", progress: startProgress } : starting}
              // Behind the sessions page or the settings page, the quit question or the delete question the screen is out of reach, and a
              // trust question it opened could not be seen or answered: Add project and Ctrl+O do nothing meanwhile.
              onChooseFolder={onPage || behind || quitAsk ? undefined : chooseFolder}
              onAddProject={addProject}
              away={away}
              onStop={() => startStop.current?.stop()}
              adding={adding}
              onPassAddFailure={() => setAdding((was) => (was?.state === "failed" ? undefined : was))}
            />
          </ErrorBoundary>
        </main>
      ) : (
        <main className="stage stage-empty" hidden={onPage} inert={behind || onPage}>No session open. Start one from a project in the rail.</main>
      )}
      </div>
      <Workbench
        open={workbench.open}
        width={workbench.width}
        eases={workbench.eases}
        inert={behind}
        onResize={workbench.resize}
        onClose={() => workbench.setOpen(false)}
        focusAfterClose={() => document.querySelector<HTMLElement>(".rail-arrange")}
      >
        {/* Until the reader has the new session's lines the pane shows none of the one left (FLOW-READ-RAW-LOG-R04). */}
        <DeveloperPane session={shownId} view={rawLog?.session === shownId ? rawLog : undefined} backend={rawLogBackend}
          attachedTo={attachedTo}
          open={workbench.open}
          onOpenDiagnosticLog={shell?.openDiagnosticLog}
        />
      </Workbench>
      </div>
      {cleanUp.view.run && (
        <CleanUpQuestion run={cleanUp.view.run} onConfirm={cleanUp.actions.confirm} onCancel={cleanUp.actions.cancel} onStop={cleanUp.actions.stop} />
      )}
      {deleteAsk && deleteAsk.phase.kind !== "deleted" && (
        <DeleteQuestion
          title={deleteAsk.title}
          phase={deleteAsk.phase}
          onConfirm={() => void deleteAsk.run?.confirm()}
          onTryAgain={() => (deleteAsk.run ? void deleteAsk.run.start() : askDelete(deleteAsk.id, deleteAsk.fromPage))}
          onCancel={cancelDelete}
        />
      )}
    </div>
  );
}
