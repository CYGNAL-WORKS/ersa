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

/// The key a session's progress line is kept under in the tile hub, beside
/// its tile: a newline is never part of a session id.
pub(super) fn progress_key(session_id: &str) -> String {
    format!("{session_id}\nsend_progress")
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
        self.tiles.forget_key(&progress_key(&self.handle.id));
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
        match bounded(async move { session.send_message(&message).await }).await {
            Ok(()) => copies.keep(),
            // Refused before anything was written: not sent.
            Err(
                e @ SessionError::Adapter(AdapterError::NotRunning | AdapterError::Protocol(_)),
            ) => {
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
        let work = Arc::new(Work::default());
        if let Some(sending) = lock(&handle.sending).as_mut() {
            sending.work = Some(Arc::clone(&work));
        }
        let row = lock(&handle.row).clone();
        let images = self
            .adapters
            .get(&row.backend)
            .map(|adapter| adapter.image_limits())
            .unwrap_or_default();
        let scratch = Arc::clone(&handle.scratch);
        let session_id = handle.id.clone();
        let job_work = Arc::clone(&work);
        let mut job = tokio::task::spawn_blocking(move || {
            // One job per session in its scratch folder at a time, even one
            // a send gave up on that has not noticed yet.
            let _held = scratch.blocking_lock();
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

/// Publishes the session's `session.send_progress` while its send runs:
/// when the phase changes and at least every [`PROGRESS_EVERY`], at most
/// once every [`PROGRESS_GAP`]. Ends when the send does.
async fn report_progress(handle: Weak<Handle>, tiles: Arc<TileHub>, changed: Arc<Notify>) {
    let mut last: Option<(SendProgressParams, Instant)> = None;
    loop {
        let progress = {
            let Some(handle) = handle.upgrade() else {
                return;
            };
            let sending = lock(&handle.sending);
            let Some(sending) = sending.as_ref() else {
                return;
            };
            sending.progress(&handle.id)
        };
        // Due when anything changed (the phase, or the bytes copied), and
        // every so often regardless.
        let due = match &last {
            None => true,
            Some((before, at)) => *before != progress || at.elapsed() >= PROGRESS_EVERY,
        };
        if due {
            tiles.publish(
                &progress_key(&progress.session_id),
                notification_line(
                    ersa_protocol::core_api::notification::SESSION_SEND_PROGRESS,
                    &progress,
                ),
            );
            last = Some((progress, Instant::now()));
        }
        // Looks again each gap; a phase change wakes it early, but it still
        // waits out the gap after the last line it sent.
        tokio::select! {
            () = changed.notified() => {}
            () = tokio::time::sleep(PROGRESS_GAP) => {}
        }
        if let Some((_, at)) = last {
            tokio::time::sleep(PROGRESS_GAP.saturating_sub(at.elapsed())).await;
        }
    }
}
