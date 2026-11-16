//! `session.send` with attachments (PROTO-CORE-API, Turns and attachments).
//!
//! One message is sent at a time per session. A send passes through four
//! phases: checking its attachments, copying the ones from outside the
//! workspace, waiting for a starting backend, and handing the message over.
//! Until it is handed over, `session.interrupt` cancels it
//! (PROTO-CORE-API-R25). Throughout, Core tells every client where it is
//! with `session.send_progress`, and `session.list` reports the phase.
//!
//! The file system work runs as one blocking job per message (see
//! [`crate::attachments`]). This side watches it: it gives up when the
//! message is canceled, when one step takes longer than [`FS_STEP`], or when
//! the whole job takes longer than [`FS_JOB`], and then answers at once; the
//! job notices at its next step and removes what it copied.

use std::time::Instant;

use ersa_adapter::OutgoingMessage;
use ersa_protocol::core_api::{Attachment, SendPhase, SendProgressParams};

use super::*;
use crate::admission::{Outcome, SendIs, SendStage};
use crate::attachments::{self, Prepared, Refused, Target, Work};

/// The most time one file system step of a send may take.
const FS_STEP: Duration = Duration::from_secs(30);
/// The most time all the file system work for one message may take.
const FS_JOB: Duration = Duration::from_secs(5 * 60);
/// `session.send_progress` goes out at least this often while a send runs,
const PROGRESS_EVERY: Duration = Duration::from_secs(5);
/// and at most this often.
const PROGRESS_GAP: Duration = Duration::from_secs(1);

/// What a send under way reports its progress from, kept on its
/// session's handle for as long as it reports. Admission has the slot, the
/// send's stage and whether it was cancelled (ARCH-CORE-R14); nothing is
/// decided from this.
pub(super) struct Reporting {
    /// The file system job, once one runs.
    work: Option<Arc<Work>>,
    /// Woken when the phase changes, so progress goes out promptly.
    changed: Arc<Notify>,
}

/// The phase a send at `stage` reports: checking turns into copying once
/// its copy job, `work`, starts copying.
fn phase_of(stage: SendStage, work: Option<&Arc<Work>>) -> SendPhase {
    match (stage, work) {
        (SendStage::Checking, Some(work)) if work.copying().is_some() => SendPhase::Copying,
        (SendStage::Checking, _) => SendPhase::Checking,
        (SendStage::Starting, _) => SendPhase::Starting,
        (SendStage::HandingOver, _) => SendPhase::HandingOver,
    }
}

impl Handle {
    /// The phase of the message being sent to the session, if one is: its
    /// stage as admission has it, and `copying` once its copy job, kept
    /// with what the send reports from, has begun.
    pub(super) fn send_phase(&self) -> Option<SendPhase> {
        let reporting = lock(&self.reporting);
        let stage = self.admission.send_stage(&self.id)?;
        let work = reporting.as_ref().and_then(|send| send.work.as_ref());
        Some(phase_of(stage, work))
    }
}

impl Reporting {
    fn progress(&self, session_id: &str, stage: SendStage) -> SendProgressParams {
        let phase = phase_of(stage, self.work.as_ref());
        let copied = match (phase, &self.work) {
            (SendPhase::Copying, Some(work)) => work.copying(),
            _ => None,
        };
        SendProgressParams {
            session_id: session_id.to_string(),
            phase,
            copied_bytes: copied.map(|(copied, _)| copied),
            total_bytes: copied.map(|(_, total)| total),
        }
    }
}

/// Cancels the session's send when one is under way and not yet handed
/// over; says whether it did. Admission decides, in one step: the send's
/// hand-over is another, so an interrupt cancels a send or finds it handed
/// over, never both. The send hears of it from admission, where it waits
/// to.
pub(super) fn cancel(handle: &Handle) -> bool {
    handle.admission.interrupt_send(&handle.id)
}

/// The refusal of a message to an archived session.
pub(super) fn archived() -> SessionError {
    SessionError::refused(
        ErrorReason::SessionArchived,
        "the session is archived; unarchive or resume it to send it a message",
    )
}

/// The refusal of a second message while one is being sent.
pub(super) fn in_progress() -> SessionError {
    SessionError::refused(
        ErrorReason::SendInProgress,
        "a message is already being sent to this session; wait for its answer",
    )
}

fn stopped() -> SessionError {
    SessionError::refused(
        ErrorReason::SendStopped,
        "the message was stopped before it was sent",
    )
}

/// Ends a send when dropped, however it ends: the session can send again,
/// `session.list` no longer reports it, and its progress stops.
struct Finish {
    handle: Arc<Handle>,
    tiles: Arc<TileHub>,
    ticker: AbortHandle,
    /// The task that waits in admission to hear the send was cancelled.
    waker: AbortHandle,
    /// The send's slot in admission, freed here.
    slot: Option<UnderWay>,
}

impl Drop for Finish {
    fn drop(&mut self) {
        self.ticker.abort();
        self.waker.abort();
        // Under the lock progress is published under: nothing of this send
        // is published after.
        let mut reporting = lock(&self.handle.reporting);
        reporting.take();
        // A progress line not yet delivered would arrive after the answer.
        // Forgotten before the slot is free, so the first line of whoever
        // reports for the session next is never the one forgotten.
        self.tiles.forget_one(&self.handle.id, Latest::SendProgress);
        // The slot is free: a send waiting its turn behind this one, held
        // in admission until now, reports in its place.
        drop(self.slot.take());
    }
}

/// A message's copies until it is sent: dropped unsent, they are removed.
struct Copies(Option<Prepared>);

impl Copies {
    /// The message was handed over, or may have been: its copies stay.
    fn keep(&mut self) {
        self.0.take();
    }
}

impl Drop for Copies {
    fn drop(&mut self) {
        if let Some(prepared) = self.0.take() {
            tokio::task::spawn_blocking(move || prepared.discard());
        }
    }
}

/// The refusal of a send while an archive is under way: no `reason`, since
/// the archive may still fail or be refused (PROTO-CORE-API-R85).
pub(super) fn being_archived() -> SessionError {
    SessionError::InvalidState(
        "the session is being archived; unarchive or resume it to send it a message".into(),
    )
}

/// Reports a `session.send` that is still waiting its turn: behind an
/// earlier request on its session, or for a run winding down
/// (PROTO-CORE-API-R90). From when Core receives the send until it has its
/// session's slot, `session.send_progress` says `checking` as [`Pacer`]
/// paces it, except while another send on the session is under way, which
/// reports for the session itself. Once the send has its slot its own
/// progress takes over. Dropped before the send's answer goes out.
///
/// Its reports are never forgotten as a send's own progress is at its end:
/// the session's latest report may by then be another send's, and a
/// `checking` that arrives after an answer changes nothing, since the
/// Interface shows nothing for `checking` and the Shell counts a report
/// only toward a send still waiting on that session.
pub struct Waiting {
    handle: Arc<Handle>,
    /// Raised, under the lock reports are published under, when the
    /// reports must stop.
    done: Arc<AtomicBool>,
    ticker: AbortHandle,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.ticker.abort();
        // Under the lock the reports are published under, so none follows.
        let _reporting = lock(&self.handle.reporting);
        self.done.store(true, Ordering::SeqCst);
    }
}

impl SessionManager {
    /// Starts reporting a `session.send` to session `id` that Core has just
    /// received; the reports end when `admitted` rises (the send has its
    /// slot) or the returned [`Waiting`] is dropped. `None` for a session
    /// Core does not know, whose send is refused at once.
    pub fn report_waiting(&self, id: &str, admitted: watch::Receiver<bool>) -> Option<Waiting> {
        let handle = self.handle(id).ok()?;
        let done = Arc::new(AtomicBool::new(false));
        let ticker = tokio::spawn(report_waiting(
            Arc::downgrade(&handle),
            Arc::clone(&self.tiles),
            Arc::clone(&done),
            admitted,
        ))
        .abort_handle();
        Some(Waiting {
            handle,
            done,
            ticker,
        })
    }

    /// Sends a user message with its attachments (PROTO-CORE-API-R21 to
    /// R23, R26), answering with the turn it starts or joins once the
    /// backend has it, `None` when Core has not projected it yet.
    /// `admitted` runs once the send has its session's slot, so requests
    /// behind it on the same connection need not wait for the copying.
    ///
    /// A session that is not running is resumed first with `resumption`,
    /// its backend starting as the send's `starting` phase
    /// (FLOW-RESUME-SESSION-R02); without one, the send is refused.
    pub async fn send(
        &self,
        id: &str,
        text: String,
        attachments: Vec<Attachment>,
        admitted: impl FnOnce(),
        resumption: Option<Resumption>,
    ) -> Result<Option<u32>, SessionError> {
        let handle = self.handle(id)?;
        let resuming = !self.runs_once_settled(&handle).await?;
        let (cancel, mut canceled) = watch::channel(false);
        let changed = Arc::new(Notify::new());
        let slot = {
            let mut reporting = lock(&handle.reporting);
            // One step in admission takes the session's send slot, or says
            // why not: its own delete has its workspace or marked it; it
            // is being archived (PROTO-CORE-API-R85) or is archived
            // (FLOW-ARCHIVE-DELETE-SESSION-R02), said before it is found
            // not running so that nothing resumes it; it is not running
            // and this send cannot start it; a fork of it is starting; a
            // message is being sent already. A delete, an archive and a removal are known to
            // admission before they ask whether a message is being sent,
            // so one of the two always sees the other.
            let slot = self
                .admission
                .begin_send(id, resumption.is_some())
                .map_err(|refusal| self.refused_send(&handle, refusal))?;
            // What the send reports its progress from, stored with the slot
            // taken.
            *reporting = Some(Reporting {
                work: None,
                changed: Arc::clone(&changed),
            });
            slot
        };
        let ticker = tokio::spawn(report_progress(
            Arc::downgrade(&handle),
            Arc::clone(&self.tiles),
            Arc::clone(&changed),
        ))
        .abort_handle();
        // Waits in admission to hear that an interrupt cancelled this send,
        // and wakes it then. One cancelled before this asks is told at once.
        let waker = tokio::spawn({
            let (admission, session) = (Arc::clone(&self.admission), id.to_string());
            async move {
                let until = SendIs::Cancelled;
                let heard = admission.ask(Command::AwaitSend { session, until }).await;
                if heard == Outcome::Done {
                    cancel.send_replace(true);
                }
            }
        })
        .abort_handle();
        let _finish = Finish {
            handle: Arc::clone(&handle),
            tiles: Arc::clone(&self.tiles),
            ticker,
            waker,
            slot: Some(slot),
        };
        admitted();

        // A session to resume is refused for a folder that is gone before
        // anything is copied into it or the backend is said to be starting
        // (FLOW-RESUME-SESSION-R03); the resume then looks no more.
        if resuming {
            resume::check_folder(&handle).await?;
        }

        let mut copies = Copies(None);
        let outgoing = if attachments.is_empty() {
            Vec::new()
        } else {
            let prepared = self.prepare(&handle, attachments, &mut canceled).await?;
            let outgoing = prepared.attachments.clone();
            copies.0 = Some(prepared);
            outgoing
        };

        let live = match (handle.live(), resumption) {
            (Some(live), _) => live,
            (None, Some(resumption)) => {
                set_stage(&handle, SendStage::Starting);
                self.start_again(&handle, resumption, resume::FolderChecked::Yes)
                    .await?;
                handle.live().ok_or_else(|| handle.not_running())?
            }
            (None, None) => return Err(handle.not_running()),
        };
        if live.ready().is_none() {
            set_stage(&handle, SendStage::Starting);
        }
        let ready = tokio::select! {
            ready = live.wait_ready() => ready,
            Ok(_) = canceled.wait_for(|c| *c) => return Err(stopped()),
        };
        // Removed while this send waited: its scratch folder is gone, and
        // must not be made again.
        let _still_there = handle.row()?;
        let session = match ready {
            Ok(session) => session,
            Err(SessionError::Refused {
                reason: ErrorReason::SessionNotRunning,
                ..
            }) if lock(&handle.start_failure).is_some() => {
                // The session's run ends just after; answer once it has, so
                // `session.list` already says the session failed and the
                // sender can tell a failed start from a slow one by its
                // state. Bounded: a run that has not ended in time is
                // answered as it stands.
                let _ = wait_finished(&live, STOP_TIMEOUT).await;
                return Err(handle.not_running());
            }
            Err(e) => return Err(e),
        };
        // One step in admission, as an interrupt's cancel is: the interrupt
        // either cancelled the send before this, or finds it handed over
        // after, never both.
        if !self.admission.send_reached(id, SendStage::HandingOver) {
            // Not handed over: its session went meanwhile, or it was
            // cancelled.
            handle.row()?;
            return Err(stopped());
        }
        changed.notify_one();

        let message = OutgoingMessage {
            text,
            attachments: outgoing,
        };
        // An adapter records each line before writing it, so whether the
        // sink took a line for the backend during this call tells a refusal
        // before writing from a write that may have reached the backend.
        // Counted for this call alone: a permission answer or an interrupt
        // written meanwhile is not this message.
        let written = Arc::new(AtomicU64::new(0));
        let call = counting_host_lines(Arc::clone(&written), async move {
            session.send_message(&message).await
        });
        match bounded(call).await {
            Ok(()) => copies.keep(),
            // Refused with nothing recorded: not sent.
            Err(
                e @ SessionError::Adapter(AdapterError::NotRunning | AdapterError::Protocol(_)),
            ) if written.load(Ordering::SeqCst) == 0 => {
                return Err(e);
            }
            // A write that failed or did not finish in time, or an adapter
            // that failed: the backend may have the message.
            Err(e) => {
                copies.keep();
                return Err(SessionError::DeliveryUnknown(format!(
                    "the message was handed to the backend, but Core can't tell whether it took it: {e}"
                )));
            }
        }
        // The adapter recorded the message before writing it, so the sink
        // has it. Wait for the pipeline to project its line: the turn it
        // starts (or joins) is known then, unless the backend holds it
        // behind the running turn. Then its `user.message` comes when the
        // backend takes it in, into that turn or the next one, so its turn
        // is not known yet. One send runs at a time per session, so if any
        // message waits, this one does.
        let target = live.sink.next_index();
        let mut progress = live.progress.clone();
        let settled = matches!(
            tokio::time::timeout(SEND_SETTLE, progress.wait_for(|p| p.lines >= target)).await,
            Ok(Ok(_))
        );
        let projected = *progress.borrow();
        // Sent, but not projected in time, or held: the turn is not known,
        // and the `user.message` event follows (PROTO-CORE-API, Sending).
        Ok((settled && !projected.holding).then(|| projected.turn.max(1)))
    }

    /// Checks and copies a message's attachments on a blocking thread,
    /// watching the job for a cancel and its time limits.
    async fn prepare(
        &self,
        handle: &Arc<Handle>,
        attachments: Vec<Attachment>,
        canceled: &mut watch::Receiver<bool>,
    ) -> Result<Prepared, SessionError> {
        let work = Arc::new(Work::pausing(self.copy_pause));
        if let Some(reporting) = lock(&handle.reporting).as_mut() {
            reporting.work = Some(Arc::clone(&work));
        }
        let row = handle.row()?;
        let images = self.adapter_of(&row.backend)?.image_limits();
        // One job per session in its scratch folder at a time, even one a
        // send gave up on that has not noticed yet. Waited for here, never
        // on a thread: a job stuck on a file system that hangs then holds
        // one thread, not one per send behind it.
        let held = tokio::select! {
            held = tokio::time::timeout(FS_STEP, Arc::clone(&handle.scratch).lock_owned()) => {
                held.map_err(|_| SessionError::InvalidState(
                    "an earlier copy into this session's scratch folder has not finished; \
                     its file system may not be answering"
                        .into(),
                ))?
            }
            Ok(_) = canceled.wait_for(|c| *c) => return Err(stopped()),
        };
        let session_id = handle.id.clone();
        let job_work = Arc::clone(&work);
        let mut job = tokio::task::spawn_blocking(move || {
            let _held = held;
            let target = Target {
                workspace: Path::new(&row.cwd),
                session_id: &session_id,
                images,
            };
            attachments::prepare(&target, &attachments, &job_work)
        });

        let began = Instant::now();
        let (mut steps, mut stepped) = (work.steps(), Instant::now());
        let mut tick = tokio::time::interval(PROGRESS_GAP);
        let outcome = loop {
            tokio::select! {
                outcome = &mut job => break outcome,
                Ok(_) = canceled.wait_for(|c| *c) => {
                    abandon(job, &work);
                    return Err(stopped());
                }
                _ = tick.tick() => {
                    let now = work.steps();
                    if now != steps {
                        (steps, stepped) = (now, Instant::now());
                    }
                    if stepped.elapsed() >= FS_STEP || began.elapsed() >= FS_JOB {
                        let index = work.at();
                        abandon(job, &work);
                        return Err(SessionError::AttachmentRefused {
                            index,
                            reason: "a time limit was reached reading or copying it".into(),
                        });
                    }
                }
            }
        };
        match outcome {
            Ok(Ok(prepared)) => Ok(prepared),
            Ok(Err(Refused::At { index, reason })) => {
                Err(SessionError::AttachmentRefused { index, reason })
            }
            Ok(Err(Refused::Stopped)) => Err(stopped()),
            Err(e) => Err(SessionError::Internal(format!(
                "checking the attachments failed: {e}"
            ))),
        }
    }
}

/// Lets a job run on that this send no longer waits for: it stops at its
/// next step and removes what it copied, and a job that finished meanwhile
/// has its copies removed here.
fn abandon(job: tokio::task::JoinHandle<Result<Prepared, Refused>>, work: &Work) {
    work.stop();
    tokio::spawn(async move {
        if let Ok(Ok(prepared)) = job.await {
            let _ = tokio::task::spawn_blocking(move || prepared.discard()).await;
        }
    });
}

/// Tells admission the session's send has come to `stage`, and has its
/// progress go out.
fn set_stage(handle: &Handle, stage: SendStage) {
    handle.admission.send_reached(&handle.id, stage);
    if let Some(reporting) = lock(&handle.reporting).as_ref() {
        reporting.changed.notify_one();
    }
}

/// Publishes the session's `session.send_progress` while its send runs,
/// as [`Pacer`] paces it. Ends when the send does.
async fn report_progress(handle: Weak<Handle>, tiles: Arc<TileHub>, changed: Arc<Notify>) {
    let mut pacer = Pacer::default();
    loop {
        {
            let Some(handle) = handle.upgrade() else {
                return;
            };
            // Published under the lock the send's end takes, so no line is
            // left to arrive after its answer.
            let reporting = lock(&handle.reporting);
            let Some(reporting) = reporting.as_ref() else {
                return;
            };
            let Some(stage) = handle.admission.send_stage(&handle.id) else {
                return;
            };
            let progress = reporting.progress(&handle.id, stage);
            publish_if_due(&mut pacer, &tiles, &progress);
        }
        pacer.wait(&changed).await;
    }
}

/// Publishes a send's progress to every connection when `pacer` says it is
/// due.
fn publish_if_due(pacer: &mut Pacer, tiles: &TileHub, progress: &SendProgressParams) {
    if pacer.due(progress) {
        tiles.publish(
            &progress.session_id,
            Latest::SendProgress,
            notification_line(
                ersa_protocol::core_api::notification::SESSION_SEND_PROGRESS,
                progress,
            ),
        );
    }
}

/// Publishes `checking` for a send waiting its turn (see [`Waiting`]).
async fn report_waiting(
    handle: Weak<Handle>,
    tiles: Arc<TileHub>,
    done: Arc<AtomicBool>,
    admitted: watch::Receiver<bool>,
) {
    let mut pacer = Pacer::default();
    // Nothing changes a waiting send's phase; the pacer waits on its clock.
    let unchanged = Notify::new();
    let Some(handle_now) = handle.upgrade() else {
        return;
    };
    let (admission, session) = (Arc::clone(&handle_now.admission), handle_now.id.clone());
    drop(handle_now);
    // Waits in admission until the session's send slot is as `until` says.
    let until = |until: SendIs| {
        let session = session.clone();
        admission.ask(Command::AwaitSend { session, until })
    };
    // Asked once, and again only when it was answered: a send that takes
    // the slot from here on is heard of, however short it is.
    let mut taken = Box::pin(until(SendIs::Taken));
    loop {
        {
            let Some(handle) = handle.upgrade() else {
                return;
            };
            // Published under the lock a send takes its slot under, so a
            // send that has its slot is seen here and reports for itself.
            let _reporting = lock(&handle.reporting);
            if done.load(Ordering::SeqCst) || *admitted.borrow() {
                return;
            }
            if handle.admission.send_stage(&handle.id).is_none() {
                let waiting = SendProgressParams {
                    session_id: handle.id.clone(),
                    phase: SendPhase::Checking,
                    copied_bytes: None,
                    total_bytes: None,
                };
                publish_if_due(&mut pacer, &tiles, &waiting);
            }
        }
        tokio::select! {
            // First, so a send that has the slot is waited for, never
            // polled.
            biased;
            heard = &mut taken => {
                // While another send reports for the session, this one
                // waits, held in admission, for it to end. That send's end
                // forgets its last report, and has this one report at once,
                // so the session's reports keep their pace. A session that
                // is gone has nothing to report for.
                if heard != Outcome::Done || until(SendIs::Free).await != Outcome::Done {
                    return;
                }
                pacer = Pacer::default();
                taken = Box::pin(until(SendIs::Taken));
            }
            () = pacer.wait(&unchanged) => {}
        }
    }
}

/// When a send's progress goes out (PROTO-CORE-API-R26): whenever it
/// changes (the phase, or the bytes copied) and at least every
/// [`PROGRESS_EVERY`] regardless, at most once every [`PROGRESS_GAP`].
/// Kept apart from the session so a test can drive it on a paused clock.
#[derive(Debug, Default)]
pub struct Pacer {
    /// The last progress sent, and when.
    last: Option<(SendProgressParams, tokio::time::Instant)>,
}

impl Pacer {
    /// Whether `progress` should go out now; when it should, it counts as
    /// sent.
    pub fn due(&mut self, progress: &SendProgressParams) -> bool {
        let due = match &self.last {
            None => true,
            Some((before, at)) => before != progress || at.elapsed() >= PROGRESS_EVERY,
        };
        if due {
            self.last = Some((progress.clone(), tokio::time::Instant::now()));
        }
        due
    }

    /// Waits until progress should be looked at again: within a gap, for
    /// bytes copied; at once when `changed` wakes it; and at the repeat's
    /// deadline, scheduled rather than found at the next look, so it is
    /// never late. It still waits out the gap after the last line sent.
    pub async fn wait(&self, changed: &Notify) {
        let Some((_, at)) = &self.last else {
            return;
        };
        tokio::select! {
            () = changed.notified() => {}
            () = tokio::time::sleep(PROGRESS_GAP) => {}
            () = tokio::time::sleep_until(*at + PROGRESS_EVERY) => {}
        }
        tokio::time::sleep_until(*at + PROGRESS_GAP).await;
    }
}
