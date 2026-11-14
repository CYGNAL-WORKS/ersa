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
use crate::attachments::{self, Prepared, Refused, Target, Work};

/// The most time one file system step of a send may take.
const FS_STEP: Duration = Duration::from_secs(30);
/// The most time all the file system work for one message may take.
const FS_JOB: Duration = Duration::from_secs(5 * 60);
/// `session.send_progress` goes out at least this often while a send runs,
const PROGRESS_EVERY: Duration = Duration::from_secs(5);
/// and at most this often.
const PROGRESS_GAP: Duration = Duration::from_secs(1);

/// A send under way, kept on its session's handle.
pub(super) struct Sending {
    /// The phase, except that checking turns into copying once the job
    /// starts copying (see [`Sending::phase`]).
    phase: SendPhase,
    /// Raised by `session.interrupt` to cancel the send.
    cancel: watch::Sender<bool>,
    /// The file system job, once one runs.
    work: Option<Arc<Work>>,
    /// Woken when the phase changes, so progress goes out promptly.
    changed: Arc<Notify>,
}

impl Sending {
    pub(super) fn phase(&self) -> SendPhase {
        match (&self.work, self.phase) {
            (Some(work), SendPhase::Checking) if work.copying().is_some() => SendPhase::Copying,
            (_, phase) => phase,
        }
    }

    fn progress(&self, session_id: &str) -> SendProgressParams {
        let phase = self.phase();
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
/// over; says whether it did.
pub(super) fn cancel(handle: &Handle) -> bool {
    match lock(&handle.sending).as_ref() {
        Some(sending) if sending.phase != SendPhase::HandingOver => {
            sending.cancel.send_replace(true);
            true
        }
        _ => false,
    }
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
}

impl Drop for Finish {
    fn drop(&mut self) {
        self.ticker.abort();
        lock(&self.handle.sending).take();
        // A progress line not yet delivered would arrive after the answer.
        self.tiles.forget_one(&self.handle.id, Latest::SendProgress);
        // A send waiting its turn behind this one reports in its place.
        self.handle.send_ended.notify_waiters();
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
fn being_archived() -> SessionError {
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
pub struct Waiting {
    handle: Arc<Handle>,
    tiles: Arc<TileHub>,
    /// Raised, under the `sending` lock, when the reports must stop.
    done: Arc<AtomicBool>,
    /// Risen once the send has its slot; its own progress reported since.
    admitted: watch::Receiver<bool>,
    ticker: AbortHandle,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.ticker.abort();
        // Under the lock the reports are published under, so none follows.
        let sending = lock(&self.handle.sending);
        self.done.store(true, Ordering::SeqCst);
        // A send answered before it had its slot may leave a report not
        // yet delivered, which would arrive after the answer: forget it. Any
        // other send waiting its turn on the session then reports again at
        // once, in case the report forgotten was its own. A send that had its
        // slot was reported by its own progress, whose end forgot its line,
        // and the latest line now belongs to whichever send comes next.
        if sending.is_none() && !*self.admitted.borrow() {
            self.tiles.forget_one(&self.handle.id, Latest::SendProgress);
            self.handle.send_ended.notify_waiters();
        }
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
            admitted.clone(),
        ))
        .abort_handle();
        Some(Waiting {
            handle,
            tiles: Arc::clone(&self.tiles),
            done,
            admitted,
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
        self.refuse_if_deleting(&handle)?;
        // Before the session is found not running: between an archive's stop
        // and its mark, the refusal is this one, and nothing resumes it
        // (PROTO-CORE-API-R85). Checked again under the `sending` lock below.
        if handle.archiving.load(Ordering::SeqCst) > 0 {
            return Err(being_archived());
        }
        let resuming = resume::current_run(&handle).await?.is_none();
        let (cancel, mut canceled) = watch::channel(false);
        let changed = Arc::new(Notify::new());
        {
            let mut sending = lock(&handle.sending);
            // Again under the lock a delete's working check reads `sending`
            // through, so a delete that claimed the workspace since the check
            // above is seen here, or sees this send.
            self.refuse_if_deleting(&handle)?;
            // Under this lock, and a fork checks `sending` after raising
            // this, so one of the two always sees the other.
            if handle.forking.load(Ordering::SeqCst) {
                return Err(SessionError::InvalidState(
                    "this session is being forked; send the message again in a moment".into(),
                ));
            }
            // Under this lock too, and an archive checks `sending` after
            // raising this, so one of the two always sees the other
            // (PROTO-CORE-API-R85).
            if handle.archiving.load(Ordering::SeqCst) > 0 {
                return Err(being_archived());
            }
            // An archived session is read-only until it is unarchived, or a
            // resume is accepted (FLOW-ARCHIVE-DELETE-SESSION-R02). Read
            // here, under the lock an archive checks `sending` through, and
            // before the session is found not running, so the refusal names
            // the reason, and an archive that finished while this send waited
            // for a run winding down is never undone by the resume below.
            if lock(&handle.row).archived {
                return Err(SessionError::refused(
                    ErrorReason::SessionArchived,
                    "the session is archived; unarchive or resume it to send it a message",
                ));
            }
            if resuming && resumption.is_none() {
                return Err(handle.not_running());
            }
            if sending.is_some() {
                return Err(SessionError::refused(
                    ErrorReason::SendInProgress,
                    "a message is already being sent to this session; wait for its answer",
                ));
            }
            *sending = Some(Sending {
                phase: SendPhase::Checking,
                cancel,
                work: None,
                changed: Arc::clone(&changed),
            });
        }
        let ticker = tokio::spawn(report_progress(
            Arc::downgrade(&handle),
            Arc::clone(&self.tiles),
            changed,
        ))
        .abort_handle();
        let _finish = Finish {
            handle: Arc::clone(&handle),
            tiles: Arc::clone(&self.tiles),
            ticker,
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
                set_phase(&handle, SendPhase::Starting);
                self.start_again(&handle, resumption, resume::FolderChecked::Yes)
                    .await?;
                handle.live().ok_or_else(|| handle.not_running())?
            }
            (None, None) => return Err(handle.not_running()),
        };
        if live.ready().is_none() {
            set_phase(&handle, SendPhase::Starting);
        }
        let ready = tokio::select! {
            ready = live.wait_ready() => ready,
            Ok(_) = canceled.wait_for(|c| *c) => return Err(stopped()),
        };
        // Removed while this send waited: its scratch folder is gone, and
        // must not be made again.
        if handle.removed.load(Ordering::SeqCst) {
            return Err(SessionError::NotFound(handle.id.clone()));
        }
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
        {
            // Under the lock, so an interrupt either cancels the send or
            // finds it handed over, never both.
            let mut sending = lock(&handle.sending);
            if *canceled.borrow() {
                return Err(stopped());
            }
            if let Some(sending) = sending.as_mut() {
                sending.phase = SendPhase::HandingOver;
                sending.changed.notify_one();
            }
        }

        let message = OutgoingMessage {
            text,
            attachments: outgoing,
        };
        // An adapter records each line before writing it, so whether the
        // sink took a line for the backend during the call tells a refusal
        // before writing from a write that may have reached the backend.
        let before = live.sink.host_lines();
        match bounded(async move { session.send_message(&message).await }).await {
            Ok(()) => copies.keep(),
            // Refused with nothing recorded: not sent.
            Err(
                e @ SessionError::Adapter(AdapterError::NotRunning | AdapterError::Protocol(_)),
            ) if live.sink.host_lines() == before => {
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
        // has it. Wait for the pipeline to project it: the turn it starts
        // (or joins) is known then.
        let target = live.sink.next_index();
        let mut progress = live.progress.clone();
        let settled = matches!(
            tokio::time::timeout(SEND_SETTLE, progress.wait_for(|p| p.lines >= target)).await,
            Ok(Ok(_))
        );
        // Sent, but not projected in time: the turn is not known, and the
        // `user.message` event follows.
        Ok(settled.then(|| progress.borrow().turn.max(1)))
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
        if let Some(sending) = lock(&handle.sending).as_mut() {
            sending.work = Some(Arc::clone(&work));
        }
        let row = lock(&handle.row).clone();
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

fn set_phase(handle: &Handle, phase: SendPhase) {
    if let Some(sending) = lock(&handle.sending).as_mut() {
        sending.phase = phase;
        sending.changed.notify_one();
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
            let sending = lock(&handle.sending);
            let Some(sending) = sending.as_ref() else {
                return;
            };
            publish_if_due(&mut pacer, &tiles, &sending.progress(&handle.id));
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
    loop {
        let Some(ended) = handle
            .upgrade()
            .map(|handle| Arc::clone(&handle.send_ended))
        else {
            return;
        };
        // Registered before `sending` is read, so an end in between still
        // wakes it.
        let mut send_ended = std::pin::pin!(ended.notified());
        send_ended.as_mut().enable();
        let other_send = {
            let Some(handle) = handle.upgrade() else {
                return;
            };
            // Published under the lock the send installs itself under, so a
            // send that has its slot is seen here and reports for itself.
            let sending = lock(&handle.sending);
            if done.load(Ordering::SeqCst) || *admitted.borrow() {
                return;
            }
            match sending.as_ref() {
                Some(_) => true,
                None => {
                    let waiting = SendProgressParams {
                        session_id: handle.id.clone(),
                        phase: SendPhase::Checking,
                        copied_bytes: None,
                        total_bytes: None,
                    };
                    publish_if_due(&mut pacer, &tiles, &waiting);
                    false
                }
            }
        };
        // While another send reports for the session, this one waits for it
        // to end. Either way, a send ending (or a report forgotten, see
        // [`Waiting`]) has this one report at once, so the session's reports
        // keep their pace.
        let ended_first = if other_send {
            send_ended.await;
            true
        } else {
            tokio::select! {
                () = pacer.wait(&unchanged) => false,
                () = send_ended.as_mut() => true,
            }
        };
        if ended_first {
            pacer = Pacer::default();
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
