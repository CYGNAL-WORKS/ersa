//! Claude Code's own sign-in, started from Ersa (PROTO-CLAUDE-CODE-STREAM,
//! Signing in; ADR 0040).
//!
//! The sign-in runs on a CLI process of its own, started as the model
//! probe's is (R84). Once the CLI has answered `initialize`, the adapter
//! asks it to start its sign-in (`claude_authenticate`), which it answers
//! with the addresses of the vendor's page. The adapter then asks to be
//! told when the sign-in ends (`claude_oauth_wait_for_completion`), and only
//! then hands Core the page's address (R85): a browser already signed in to
//! the vendor may finish within seconds of the page opening. How it ended
//! is the CLI's answer to that wait (R87).
//!
//! Nothing a sign-in's process writes is kept or logged, its stderr
//! included (PROTO-CORE-API-R163), and the process has no raw log: its
//! lines go to a sink that drops them (R90). Core is handed only the page's
//! address and, as a failure's reason, the error text of the CLI's answer
//! to one of the adapter's own requests.

use std::io;
use std::sync::Arc;

use ersa_adapter::{
    AdapterError, AdapterSession, AdapterSink, Direction, ProcessExit, SignIn, SignInEnd,
    SignInStartError, StartedSignIn, Transport, async_trait,
};
use serde_json::Value;
use tokio::sync::oneshot;

use super::{AskContext, ClaudeAdapter, ClaudeSession, Handshake, LaunchFailure};
use crate::wire::{HostMessage, HostRequest};

// What the person reads when the adapter could not start a sign-in. Each
// follows the note's own sentence that the sign-in couldn't be started
// (FLOW-SIGN-IN-R03), so each says what went wrong, in the person's terms,
// and never that again (PROTO-CORE-API-R170). One sentence each, ending
// with a period (`SignInStartError::CouldNotStart`).

/// The CLI answered `initialize` with an error.
pub(crate) const INITIALIZE_REFUSED: &str = "Claude Code refused to connect to Ersa.";
/// The process ended before the adapter had the page's address and had
/// asked to learn the sign-in's end, so the page never opened.
pub(crate) const QUIT_EARLY: &str = "Claude Code quit before its sign-in page could open.";
/// The CLI's answer to `claude_authenticate` holds no `automaticUrl` with
/// something in it.
pub(crate) const NO_ADDRESS: &str = "Claude Code didn't give Ersa the address of its sign-in page.";
/// The CLI said nothing within the adapter's own time limit. Core gives up
/// sooner (PROTO-CORE-API-R157), so through Core this is never read.
pub(crate) const NO_ANSWER: &str = "Claude Code didn't respond.";
/// The CLI's command was found and could not be run.
pub(crate) const WOULD_NOT_RUN: &str = "Ersa couldn't run Claude Code on this host.";

/// Where a sign-in's process's lines go: nowhere. A sign-in has no raw log
/// (PROTO-CLAUDE-CODE-STREAM-R90), and its stderr is not written to the
/// diagnostic log as the model probe's is (PROTO-CORE-API-R163).
struct SignInSink;

impl AdapterSink for SignInSink {
    fn record(&self, _dir: Direction, _line: String) {}

    fn stderr_line(&self, _line: String) {}

    fn process_exited(&self, _exit: io::Result<ProcessExit>) {}
}

/// Starts a sign-in on `transport` (PROTO-CLAUDE-CODE-STREAM-R84, R85, R91).
/// A start that fails leaves no process of the adapter's running: each
/// failure after the process started ends it, and waits until it has
/// exited, before saying why.
pub(super) async fn start(
    adapter: &ClaudeAdapter,
    transport: Arc<dyn Transport>,
) -> Result<StartedSignIn, SignInStartError> {
    // As the model probe starts (R84): `initialize` and no message, and
    // nothing asked about context, which a session would ask.
    let (session, _answer) = adapter
        .launch_or_say_why(
            transport,
            adapter.probe_config(),
            Arc::new(SignInSink),
            AskContext::No,
        )
        .await
        .map_err(not_launched)?;
    match ask_for_the_page(adapter, &session).await {
        Ok((url, wait)) => Ok(StartedSignIn {
            url: Some(url),
            sign_in: Box::new(ClaudeSignIn {
                session,
                wait: Wait::Pending(wait),
            }),
        }),
        Err(error) => {
            session.kill_and_wait().await;
            Err(error)
        }
    }
}

/// Asks the CLI to start its sign-in, and once it has answered with the
/// page's address, asks to be told how the sign-in ends: the address, and
/// the wait for that answer.
async fn ask_for_the_page(
    adapter: &ClaudeAdapter,
    session: &ClaudeSession,
) -> Result<(String, oneshot::Receiver<Value>), SignInStartError> {
    let could_not = |sentence: &str| SignInStartError::CouldNotStart(sentence.to_string());
    let limit = adapter.handshake_timeout;
    // With a Claude subscription, the CLI's default (R84).
    let request = HostRequest::ClaudeAuthenticate {
        login_with_claude_ai: true,
    };
    let answer = session
        .answer_within("claude_authenticate", request, limit)
        .await
        .map_err(|e| match e {
            // The process went away: its stdin broke, or its exit dropped
            // the wait.
            AdapterError::NotRunning | AdapterError::Io(_) => could_not(QUIT_EARLY),
            // The time ran out, the one other way a request fails before
            // an answer.
            AdapterError::Protocol(_) | AdapterError::UnknownRequest(_) => could_not(NO_ANSWER),
        })?;
    if answer["subtype"] != "success" {
        // The backend refused, in its own words, unchanged (R91). Words that
        // are empty or missing are passed on as empty: the note then shows
        // its own sentence alone (FLOW-SIGN-IN-R03).
        let reason = answer["error"].as_str().unwrap_or_default();
        return Err(SignInStartError::Refused(reason.to_string()));
    }
    // `automaticUrl` returns to a callback the CLI listens on, so the CLI
    // learns the outcome by itself. `manualUrl` ends on a code for a host to
    // relay, which Ersa never does (ADR 0040), so it is never read (R85).
    let url = match answer["response"]["automaticUrl"].as_str() {
        Some(url) if !url.is_empty() => url.to_string(),
        _ => return Err(could_not(NO_ADDRESS)),
    };
    // The wait goes out before the address is handed over, as recorded
    // (R85). Its answer may be minutes away, so nothing waits on it here.
    let (wait_id, wait) = session.await_answer("claude_oauth_wait_for_completion");
    let request = HostMessage::control_request(wait_id, HostRequest::ClaudeOauthWaitForCompletion);
    match tokio::time::timeout(limit, session.shared.write(&request)).await {
        Ok(Ok(())) => Ok((url, wait)),
        Ok(Err(_)) => Err(could_not(QUIT_EARLY)),
        Err(_) => Err(could_not(NO_ANSWER)),
    }
}

/// Why the process did not get through `initialize`, for the person.
fn not_launched(failure: LaunchFailure) -> SignInStartError {
    let sentence = match failure {
        // Core says so in its own words (PROTO-CORE-API-R157).
        LaunchFailure::NotStarted(AdapterError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
            return SignInStartError::NotFound;
        }
        LaunchFailure::NotStarted(_) => WOULD_NOT_RUN,
        // The stderr the CLI wrote on its way out is never quoted: it is
        // the sign-in's process's (PROTO-CORE-API-R163).
        LaunchFailure::Handshake { failure, stderr: _ } => match failure {
            Handshake::Refused(_) => INITIALIZE_REFUSED,
            Handshake::Exited | Handshake::Unsent(_) => QUIT_EARLY,
            Handshake::Silent { .. } => NO_ANSWER,
        },
    };
    SignInStartError::CouldNotStart(sentence.to_string())
}

/// The error text of an answer that is not `success`, unchanged, when
/// there is any to give.
fn error_text(answer: &Value) -> Option<String> {
    answer["error"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .map(String::from)
}

/// A sign-in under way: its process, and the wait for the CLI to say how
/// it ended.
struct ClaudeSignIn {
    session: ClaudeSession,
    wait: Wait,
}

enum Wait {
    /// The CLI's first answer to `claude_oauth_wait_for_completion`, still
    /// to come. The read loop drops the sender when the process exits.
    Pending(oneshot::Receiver<Value>),
    /// How it ended, once known.
    Ended(SignInEnd),
}

#[async_trait]
impl SignIn for ClaudeSignIn {
    /// How the sign-in ended (PROTO-CLAUDE-CODE-STREAM-R87): signed in when
    /// the CLI answers the wait with `success`, failed with the CLI's own
    /// words when it answers with an error, and failed with no reason when
    /// the process ends with the wait unanswered. A sign-in nobody
    /// completes is never answered, so this waits for as long as Core lets
    /// it. Nothing of a `success` answer is read: it names the account.
    ///
    /// Cancellation-safe: the wait is a receiver held here, and a call
    /// dropped part way leaves it as it was.
    async fn finished(&mut self) -> SignInEnd {
        if let Wait::Pending(answer) = &mut self.wait {
            let end = match answer.await {
                Ok(answer) if answer["subtype"] == "success" => SignInEnd::SignedIn,
                Ok(answer) => SignInEnd::Failed(error_text(&answer)),
                Err(_) => SignInEnd::Failed(None),
            };
            self.wait = Wait::Ended(end);
        }
        match &self.wait {
            Wait::Ended(end) => end.clone(),
            Wait::Pending(_) => unreachable!("the wait has just ended"),
        }
    }

    /// Closes the process's stdin and returns once it has exited, ending
    /// it should it linger, as a session's stop does.
    async fn stop(&mut self) {
        // A session's stop never fails; it ends the process either way.
        let _ = self.session.stop().await;
    }
}
