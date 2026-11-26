//! A backend's sign-in (PROTO-CORE-API, Signing in; ADR 0040): Core has the
//! backend's adapter start the backend's own sign-in, waits to learn how it
//! ended, and says so to every connection. What the sign-in exchanges is
//! the backend's: Core passes the page's address to the Interface to open,
//! and writes none of it anywhere (PROTO-CORE-API-R163).
//!
//! A sign-in is something under way in Core, so admission decides it
//! (ADR 0037): one is under way for a backend on a host from the step that
//! admits it until Core has let go of everything it started for it, and
//! admission decides whether a cancel came before its start. What Core
//! keeps for it meanwhile, the [`Handle`], is attached to it there, so a
//! second request for the same backend finds the first in the same step
//! that refuses it, and joins it.
//!
//! For everyone but the requests waiting on its start, a sign-in is under
//! way once its start has answered: only then does `backend.list` carry it,
//! and only one that started ends with `backend.sign_in_ended`. One that
//! never starts is answered `invalid_state` and nothing more, so no window
//! is left showing a sign-in nobody will say has ended.
//!
//! The sign-in runs in a task of its own, not in the request that asked for
//! it: a window that closes while a person signs in does not end it.

use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::{Adapter, AdapterError, SignInEnd, StartedSignIn};
use ersa_protocol::core_api::{SignInEndedParams, SignInOutcome, SigningIn, notification};
use tokio::sync::watch;

use crate::admission::{Attached, Refusal, SignInOf, UnderWay};
use crate::api::ApiError;
use crate::state::Core;

/// How long a sign-in may take to start before Core stops what it started
/// for it and answers `invalid_state` (PROTO-CORE-API-R157): inside the 30
/// seconds the Shell waits for most answers.
pub const SIGN_IN_START_LIMIT: Duration = Duration::from_secs(20);

/// How long a sign-in may run before Core ends it as `timed_out`
/// (PROTO-CORE-API-R161): Core's own bound on something it started, long
/// enough for a person to find a password and a second factor.
pub const SIGN_IN_LIMIT: Duration = Duration::from_secs(10 * 60);

/// How long Core waits for a sign-in's own stop before it lets go of it
/// anyway. Letting go ends everything the sign-in started, since it was
/// started through the transport that kills a process's whole tree when
/// Core lets go of it.
pub const SIGN_IN_STOP_LIMIT: Duration = Duration::from_secs(10);

/// How a sign-in's start was answered: the address of the vendor's page,
/// when the adapter handed one over, or why it did not start.
pub type Started = Result<Option<String>, ApiError>;

/// What Core keeps for a sign-in under way, attached to it in admission.
pub struct Handle {
    /// `None` until the start is answered. Its sender belongs to the
    /// sign-in's task, so a task that died leaves it closed, not waiting.
    started: watch::Receiver<Option<Started>>,
    /// Rises once nothing Core started for the sign-in is left running.
    stopped: watch::Receiver<bool>,
    /// Raised by `backend.cancel_sign_in`, once admission has the sign-in
    /// stopping.
    cancel: watch::Sender<bool>,
}

impl Handle {
    /// How the start was answered, once it was.
    async fn started(&self) -> Started {
        let mut started = self.started.clone();
        match started.wait_for(Option::is_some).await {
            Ok(answer) => answer.clone().expect("waited for an answer"),
            Err(_) => Err(ApiError::internal("the sign-in ended inside Core")),
        }
    }

    /// The sign-in as `backend.list` carries it, once it has started, with
    /// the page's address when the start answered one (PROTO-CORE-API-R158).
    fn signing_in(&self) -> Option<SigningIn> {
        match &*self.started.borrow() {
            Some(Ok(url)) => Some(SigningIn { url: url.clone() }),
            Some(Err(_)) | None => None,
        }
    }

    /// Tells the sign-in's task to stop, and waits until nothing it started
    /// is left (PROTO-CORE-API-R160, R162): not for the rest of its ending,
    /// such as a detection of the backend, which may take a while.
    async fn stop(&self) {
        self.cancel.send_replace(true);
        self.until_stopped().await;
    }

    /// Waits until nothing the sign-in started is left, or its task is gone.
    async fn until_stopped(&self) {
        let mut stopped = self.stopped.clone();
        let _ = stopped.wait_for(|stopped| *stopped).await;
    }
}

/// The sign-in under way for `of` as `backend.list` carries it: `None`
/// unless one has started (PROTO-CORE-API-R158).
pub fn signing_in(core: &Core, of: &SignInOf) -> Option<SigningIn> {
    let handle = core.admission.sign_in(of)?.downcast::<Handle>().ok()?;
    handle.signing_in()
}

/// Starts `adapter`'s sign-in for `of`, or joins the one starting or
/// started for it, and answers once it has started, as the first request
/// for it was answered (PROTO-CORE-API-R155, R156). While one is ending it
/// waits, and then starts afresh.
pub async fn begin(core: &Arc<Core>, of: SignInOf, adapter: Arc<dyn Adapter>) -> Started {
    // R157's 20 seconds count from the request, a wait for a sign-in that
    // is ending included.
    let deadline = tokio::time::Instant::now() + core.config.sign_in_start_limit;
    let (started_tx, started) = watch::channel(None);
    let (stopped_tx, stopped) = watch::channel(false);
    let (cancel, canceled) = watch::channel(false);
    let handle = Arc::new(Handle {
        started,
        stopped,
        cancel,
    });
    let attach = || Arc::clone(&handle) as Attached;
    match core.admission.begin_sign_in(&of, deadline, attach).await {
        Ok(under_way) => {
            let task = Task {
                core: Arc::clone(core),
                of,
                adapter,
                deadline,
                under_way,
                started: started_tx,
                stopped: stopped_tx,
                canceled,
            };
            tokio::spawn(task.run());
            handle.started().await
        }
        Err((Refusal::SigningIn, Some(first))) => match first.downcast::<Handle>() {
            Ok(first) => first.started().await,
            Err(_) => Err(ApiError::internal("a sign-in under way is not one")),
        },
        Err((Refusal::Closing, _)) => Err(ApiError::invalid_state(
            "Core is shutting down; no sign-in can start",
        )),
        // The one before it took the whole limit to end.
        Err((Refusal::GaveUp, _)) => Err(too_slow(core, adapter.as_ref())),
        Err((refusal, _)) => Err(ApiError::internal(format!(
            "admission refused a sign-in: {refusal:?}"
        ))),
    }
}

/// Stops the sign-in under way for `of`, if one is, and returns once
/// nothing it started is left (PROTO-CORE-API-R160).
pub async fn cancel(core: &Core, of: &SignInOf) {
    let Some(attached) = core.admission.cancel_sign_in(of) else {
        return;
    };
    if let Ok(handle) = attached.downcast::<Handle>() {
        handle.stop().await;
    }
}

/// Waits, at shutdown, until no sign-in under way has anything it started
/// left running (PROTO-CORE-API-R162). Each sees the shutdown itself and
/// stops; this waits at most a stop's limit, after which Core's exit ends
/// what is left.
pub async fn stopped_at_shutdown(core: &Core) {
    let handles = core.admission.all_sign_ins().into_iter();
    let handles: Vec<Arc<Handle>> = handles
        .filter_map(|attached| attached.downcast::<Handle>().ok())
        .collect();
    let all = async {
        for handle in handles {
            handle.until_stopped().await;
        }
    };
    let _ = tokio::time::timeout(SIGN_IN_STOP_LIMIT, all).await;
}

/// One sign-in, from its admission until it has ended.
struct Task {
    core: Arc<Core>,
    of: SignInOf,
    adapter: Arc<dyn Adapter>,
    /// When the start's time is up, counted from the request.
    deadline: tokio::time::Instant,
    /// The sign-in as admission has it: dropped once Core has let go of
    /// everything it started, which takes `signing_in` out of
    /// `backend.list`.
    under_way: UnderWay,
    started: watch::Sender<Option<Started>>,
    stopped: watch::Sender<bool>,
    canceled: watch::Receiver<bool>,
}

/// Why Core stopped waiting for a sign-in's start or end.
enum Interrupted {
    Canceled,
    ShutDown,
}

impl Interrupted {
    /// What the requests waiting on the start are told when it never
    /// started.
    fn before_start(&self, adapter: &dyn Adapter) -> ApiError {
        ApiError::invalid_state(match self {
            Interrupted::Canceled => format!("The sign-in to {} was canceled.", adapter.label()),
            Interrupted::ShutDown => "Core is shutting down; no sign-in can start".to_string(),
        })
    }
}

impl Task {
    async fn run(self) {
        let Task {
            core,
            of,
            adapter,
            deadline,
            under_way,
            started,
            stopped,
            mut canceled,
        } = self;
        // Whichever way this task ends, what the sign-in started is gone by
        // then: say so, so a cancel or a shutdown waiting on it goes on.
        let _stopped = RaiseOnDrop(&stopped);
        // The handler answered `transport_error` for a host Core cannot
        // reach, so this is the local host.
        let Some(transport) = core.session_transport_for(&of.host) else {
            started.send_replace(Some(Err(ApiError::internal("no transport for the host"))));
            return;
        };
        let start = tokio::time::timeout_at(deadline, adapter.start_sign_in(transport));
        // A start that is dropped, whichever way, drops what it started,
        // which the transport ends, tree and all (R157, R162).
        let start = tokio::select! {
            start = start => start,
            interrupted = interrupted(&core, &mut canceled) => {
                started.send_replace(Some(Err(interrupted.before_start(adapter.as_ref()))));
                return;
            }
        };
        let StartedSignIn { url, mut sign_in } = match start {
            Ok(Ok(started)) => started,
            Ok(Err(e)) => {
                started.send_replace(Some(Err(not_started(adapter.as_ref(), e))));
                return;
            }
            Err(_) => {
                started.send_replace(Some(Err(too_slow(&core, adapter.as_ref()))));
                return;
            }
        };
        // Admission decides whether a cancel came first: then it never
        // started for anyone, and what it started is stopped.
        if !core.admission.sign_in_started(&of) {
            let _ = tokio::time::timeout(SIGN_IN_STOP_LIMIT, sign_in.stop()).await;
            drop(sign_in);
            let canceled = Interrupted::Canceled.before_start(adapter.as_ref());
            started.send_replace(Some(Err(canceled)));
            return;
        }
        started.send_replace(Some(Ok(url)));

        // Its time is counted from its start (R161).
        let limit = core.config.sign_in_limit;
        let outcome = tokio::select! {
            end = sign_in.finished() => match end {
                SignInEnd::SignedIn => (SignInOutcome::SignedIn, None),
                SignInEnd::Failed(message) => (SignInOutcome::Failed, Some(message)),
            },
            () = tokio::time::sleep(limit) => (SignInOutcome::TimedOut, None),
            // Shutdown names no outcome of its own: Core stopped it, as a
            // cancel does.
            _ = interrupted(&core, &mut canceled) => (SignInOutcome::Canceled, None),
        };
        // From now a sign-in asked for waits for this one to end.
        core.admission.sign_in_ending(&of);
        // However it ended, nothing Core started for it is left (R162): its
        // own stop first, then letting go, which ends the rest.
        let _ = tokio::time::timeout(SIGN_IN_STOP_LIMIT, sign_in.stop()).await;
        drop(sign_in);
        stopped.send_replace(true);
        finish(&core, of, under_way, outcome).await;
    }
}

/// Raises its flag when dropped.
struct RaiseOnDrop<'a>(&'a watch::Sender<bool>);

impl Drop for RaiseOnDrop<'_> {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

/// Waits until the sign-in is canceled or Core shuts down.
async fn interrupted(core: &Core, canceled: &mut watch::Receiver<bool>) -> Interrupted {
    tokio::select! {
        // `Err` means the handle is gone, which happens only once the
        // sign-in has ended; a cancel is as good an end as any then.
        _ = canceled.wait_for(|canceled| *canceled) => Interrupted::Canceled,
        () = core.shutdown_requested() => Interrupted::ShutDown,
    }
}

/// Ends a sign-in that started: detects the backend again before saying it
/// signed in (R159), lets admission end it, so `backend.list` no longer
/// carries it, and then says how it ended to every connection past
/// `core.hello`. In that order, a window that reads the backends once told
/// reads them as they are now, and one that connects after the
/// notification went out reads them so too.
async fn finish(
    core: &Core,
    of: SignInOf,
    under_way: UnderWay,
    (outcome, message): (SignInOutcome, Option<String>),
) {
    if outcome == SignInOutcome::SignedIn {
        core.detect_again(&of.backend).await;
    }
    drop(under_way);
    let params = SignInEndedParams {
        host_id: of.host,
        name: of.backend,
        outcome,
        message,
    };
    core.sessions
        .notify_all(notification::BACKEND_SIGN_IN_ENDED, &params);
}

/// The answer for a sign-in that has not started within its limit (R157).
fn too_slow(core: &Core, adapter: &dyn Adapter) -> ApiError {
    ApiError::invalid_state(format!(
        "The sign-in to {} didn't start within {} seconds.",
        adapter.label(),
        core.config.sign_in_start_limit.as_secs()
    ))
}

/// The answer for a sign-in whose adapter did not start it (R157): the
/// backend's own reason when it refused, else what went wrong, each as
/// `invalid_state`.
fn not_started(adapter: &dyn Adapter, error: AdapterError) -> ApiError {
    match error {
        AdapterError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => {
            ApiError::invalid_state(format!(
                "{} isn't installed on this host: its command wasn't found.",
                adapter.label()
            ))
        }
        AdapterError::Protocol(reason) => ApiError::invalid_state(reason),
        other => ApiError::invalid_state(format!(
            "The sign-in to {} couldn't start: {other}",
            adapter.label()
        )),
    }
}
