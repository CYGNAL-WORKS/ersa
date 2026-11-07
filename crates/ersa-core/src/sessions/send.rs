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
    SessionError::InvalidState("the message was stopped before it was sent".into())
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

impl SessionManager {
    /// Sends a user message with its attachments (PROTO-CORE-API-R21 to
    /// R23, R26), answering with the turn it starts or joins once the
    /// backend has it, `None` when Core has not projected it yet.
    /// `admitted` runs once the send has its session's slot, so requests
    /// behind it on the same connection need not wait for the copying.
    pub async fn send(
        &self,
        id: &str,
        text: String,
        attachments: Vec<Attachment>,
        admitted: impl FnOnce(),
    ) -> Result<Option<u32>, SessionError> {
        let handle = self.handle(id)?;
        self.refuse_if_deleting(&handle)?;
        // An archived session is read-only until it is unarchived, or a resume is accepted
        // (FLOW-ARCHIVE-DELETE-SESSION-R02). Said before anything else, so
        // the refusal names the reason rather than that it is not running.
        if lock(&handle.row).archived {
            return Err(SessionError::InvalidState(
                "the session is archived; unarchive or resume it to send it a message".into(),
            ));
        }
        let live = handle.live().ok_or_else(|| handle.not_running())?;
        let (cancel, mut canceled) = watch::channel(false);
        let changed = Arc::new(Notify::new());
        {
            let mut sending = lock(&handle.sending);
            if sending.is_some() {
                return Err(SessionError::InvalidState(
                    "a message is already being sent to this session; wait for its answer".into(),
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

        let mut copies = Copies(None);
        let outgoing = if attachments.is_empty() {
            Vec::new()
        } else {
            let prepared = self.prepare(&handle, attachments, &mut canceled).await?;
            let outgoing = prepared.attachments.clone();
            copies.0 = Some(prepared);
            outgoing
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
            Err(SessionError::InvalidState(_)) if lock(&handle.start_failure).is_some() => {
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
        let images = self
            .adapters
            .get(&row.backend)
            .map(|adapter| adapter.image_limits())
            .unwrap_or_default();
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
            let progress = sending.progress(&handle.id);
            if pacer.due(&progress) {
                tiles.publish(
                    &handle.id,
                    Latest::SendProgress,
                    notification_line(
                        ersa_protocol::core_api::notification::SESSION_SEND_PROGRESS,
                        &progress,
                    ),
                );
            }
        }
        pacer.wait(&changed).await;
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
