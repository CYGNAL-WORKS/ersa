//! The Claude Code adapter's sign-in (PROTO-CLAUDE-CODE-STREAM, Signing in)
//! against a stand-in CLI. The stand-in plays the lines Claude Code 2.1.330
//! wrote in the sign-in recordings (`2.1.330/sign-in/`), read from the
//! fixture files, so a recording made again changes what these tests
//! replay; the lines no recording holds (refusals, early exits, stray
//! lines) are written here, on top of a recording.
//!
//! No test here starts the real CLI, or any sign-in on it.

mod common;

use std::io;
use std::sync::{Arc, OnceLock};

use common::stand_in::{Script, StandIn, Step, parse, without_id};
use common::{Log, fixtures_root, sign_in_recording};
use ersa_adapter::recording::Recording;
use ersa_adapter::{
    Adapter, Process, ProcessSpec, SignIn, SignInEnd, SignInStartError, StartedSignIn, Transport,
    async_trait,
};
use ersa_adapter_claude::ClaudeAdapter;
use serde_json::{Value, json};

/// The version the sign-in recordings were made on.
const VERSION: &str = "2.1.330";

/// The recordings of a sign-in stopped at once, one nobody completed, and
/// one a person finished.
const STOPPED: &str = "00-sign-in-stopped";
const ABANDONED: &str = "03-sign-in-abandoned";
const FINISHED: &str = "06-sign-in-finished";

const AUTHENTICATE: &str = "claude_authenticate";
const WAIT: &str = "claude_oauth_wait_for_completion";
const INITIALIZE: &str = "initialize";

/// What the adapter says, in its own words, for each start it could not
/// make (PROTO-CLAUDE-CODE-STREAM-R86).
const INITIALIZE_REFUSED: &str = "Claude Code refused to connect to Ersa.";
const QUIT_EARLY: &str = "Claude Code quit before it gave Ersa its sign-in page.";
const NO_ADDRESS: &str = "Claude Code didn't give Ersa the address of its sign-in page.";
const WOULD_NOT_RUN: &str = "Ersa couldn't run Claude Code on this host.";

/// Everything this binary logs, at every level: stricter than Core's own
/// diagnostic log, which keeps `info` and above. Installed by every test
/// before the adapter runs, since `tracing` remembers whether a place that
/// logs is wanted from the first time it is reached.
fn log() -> Log {
    static LOG: OnceLock<Log> = OnceLock::new();
    LOG.get_or_init(|| {
        let log = Log::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(log.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::subscriber::set_global_default(subscriber).unwrap();
        log
    })
    .clone()
}

fn recorded(name: &str) -> Script {
    Script::recorded(&sign_in_recording(VERSION, name))
}

/// Starts a sign-in with the real adapter on a stand-in playing `script`.
async fn start(script: Script) -> (StandIn, Result<StartedSignIn, SignInStartError>) {
    log();
    let stand_in = StandIn::new([script]);
    let started = ClaudeAdapter::new()
        .start_sign_in(Arc::new(stand_in.clone()))
        .await;
    (stand_in, started)
}

/// A sign-in that must have started.
fn started(started: Result<StartedSignIn, SignInStartError>) -> StartedSignIn {
    started.unwrap_or_else(|e| panic!("the sign-in did not start: {e:?}"))
}

/// A start that must have failed.
fn failed(started: Result<StartedSignIn, SignInStartError>) -> SignInStartError {
    match started {
        Ok(_) => panic!("the sign-in started"),
        Err(e) => e,
    }
}

/// The field `field` of the recorded answer to `claude_authenticate`.
fn recorded_address(name: &str, field: &str) -> String {
    sign_in_recording(VERSION, name)
        .stdout()
        .iter()
        .find_map(|line| {
            parse(line)["response"]["response"][field]
                .as_str()
                .map(String::from)
        })
        .unwrap_or_else(|| panic!("{name} answers no {field}"))
}

/// How the sign-in ended, failing the test should it not end within a
/// minute: generous for a busy machine, and never a pause a test waits out.
async fn end(sign_in: &mut Box<dyn SignIn>) -> SignInEnd {
    tokio::time::timeout(std::time::Duration::from_secs(60), sign_in.finished())
        .await
        .expect("the sign-in ended")
}

/// How the sign-in has ended by now, polled once: `None` while it has not.
async fn ended_yet(sign_in: &mut Box<dyn SignIn>) -> Option<SignInEnd> {
    tokio::select! {
        biased;
        end = sign_in.finished() => Some(end),
        () = std::future::ready(()) => None,
    }
}

/// The request subtypes the adapter wrote to process `n`, in order.
fn requests(stand_in: &StandIn, n: usize) -> Vec<String> {
    stand_in
        .host_lines(n)
        .iter()
        .map(|line| {
            let line = parse(line);
            format!(
                "{}/{}",
                line["type"].as_str().unwrap_or("?"),
                line["request"]["subtype"].as_str().unwrap_or("")
            )
        })
        .collect()
}

/// Stops the sign-in and checks its process is gone, as closing stdin ends
/// the recorded CLI (fixture 13-sign-in-closed-while-waiting).
async fn stop(stand_in: &StandIn, mut sign_in: Box<dyn SignIn>) {
    sign_in.stop().await;
    assert_eq!(
        stand_in.running(),
        0,
        "the sign-in's process is still running"
    );
    assert_eq!(stand_in.exit(0).and_then(|e| e.code), Some(0));
}

/// For each recording, the adapter writes the requests the recorder wrote,
/// in that order, and nothing else: `initialize` and no message, then
/// `claude_authenticate` with `loginWithClaudeAi: true`, then the wait,
/// which is already written when the address is handed over. The address
/// is the recorded `automaticUrl`, byte for byte, and never `manualUrl`;
/// `claude_oauth_callback` is never written.
// verifies: PROTO-CLAUDE-CODE-STREAM-R84
// verifies: PROTO-CLAUDE-CODE-STREAM-R85
#[tokio::test]
async fn each_recorded_sign_in_is_asked_for_as_the_recorder_asked() {
    for name in [STOPPED, ABANDONED, FINISHED] {
        let script = recorded(name);
        let expected: Vec<Value> = script.expected().iter().map(|l| without_id(l)).collect();
        let (stand_in, result) = start(script).await;
        let StartedSignIn { url, sign_in } = started(result);

        // Written before the address was handed over: all three, as recorded.
        let sent: Vec<Value> = stand_in
            .host_lines(0)
            .iter()
            .map(|l| without_id(l))
            .collect();
        assert_eq!(sent, expected, "{name}");
        assert_eq!(
            requests(&stand_in, 0),
            [
                "control_request/initialize",
                "control_request/claude_authenticate",
                "control_request/claude_oauth_wait_for_completion",
            ],
            "{name}"
        );
        assert_eq!(
            parse(&stand_in.host_lines(0)[1])["request"]["loginWithClaudeAi"],
            json!(true)
        );
        assert!(stand_in.failures().is_empty(), "{:#?}", stand_in.failures());

        let url = url.expect("an address is handed over");
        assert_eq!(url, recorded_address(name, "automaticUrl"), "{name}");
        assert_ne!(url, recorded_address(name, "manualUrl"), "{name}");

        stand_in.settled(0).await;
        stop(&stand_in, sign_in).await;
        // Nothing more was written, the callback least of all.
        assert_eq!(stand_in.host_lines(0).len(), 3, "{name}");
        assert!(
            stand_in
                .host_lines(0)
                .iter()
                .all(|l| !l.contains("claude_oauth_callback") && parse(l)["type"] != "user")
        );
    }
}

/// The model probe's process, as recorded on the same version.
fn probe_script() -> Script {
    let file = fixtures_root()
        .join(VERSION)
        .join("00-probe-get-settings.jsonl");
    Script::recorded(&Recording::load(&file).expect("the probe recording loads"))
}

/// A spec's arguments with the session id's value taken out.
fn args_but_the_session_id(spec: &ProcessSpec) -> (Vec<String>, String) {
    let at = spec
        .args
        .iter()
        .position(|a| a == "--session-id")
        .expect("a new session's id is passed");
    let mut args = spec.args.clone();
    let id = args.remove(at + 1);
    (args, id)
}

/// A sign-in's process starts as the model probe's does: the same program,
/// arguments, folder and environment, a new session under an id of its own.
// verifies: PROTO-CLAUDE-CODE-STREAM-R84
#[tokio::test]
async fn a_sign_in_starts_its_cli_as_the_model_probe_does() {
    log();
    let stand_in = StandIn::new([probe_script(), recorded(FINISHED)]);
    let adapter = ClaudeAdapter::new();
    let transport: Arc<dyn Transport> = Arc::new(stand_in.clone());
    adapter
        .models(Arc::clone(&transport))
        .await
        .expect("the probe answers");
    let StartedSignIn { sign_in, .. } = started(adapter.start_sign_in(transport).await);

    let [probe, signing_in] = &stand_in.spawned()[..] else {
        panic!("two processes: {:#?}", stand_in.spawned());
    };
    let (probe_args, probe_id) = args_but_the_session_id(probe);
    let (sign_in_args, sign_in_id) = args_but_the_session_id(signing_in);
    assert_eq!(sign_in_args, probe_args);
    assert_ne!(sign_in_id, probe_id, "each starts under an id of its own");
    uuid::Uuid::parse_str(&sign_in_id).expect("the id is a UUID");
    assert_eq!(signing_in.program, probe.program);
    assert_eq!(signing_in.cwd, probe.cwd);
    assert_eq!(signing_in.cwd, None, "in Core's own folder");
    assert_eq!(signing_in.env, probe.env);
    assert_eq!(signing_in.env_remove, probe.env_remove);
    // The user's settings alone, as a session of no project.
    assert!(
        sign_in_args
            .windows(2)
            .any(|w| w[0] == "--setting-sources" && w[1] == "user")
    );
    // The same `initialize` as the probe's.
    assert_eq!(
        without_id(&stand_in.host_lines(1)[0]),
        without_id(&stand_in.host_lines(0)[0])
    );
    assert!(stand_in.failures().is_empty(), "{:#?}", stand_in.failures());
    stop_second(&stand_in, sign_in).await;
}

async fn stop_second(stand_in: &StandIn, mut sign_in: Box<dyn SignIn>) {
    sign_in.stop().await;
    assert_eq!(stand_in.running(), 0);
}

/// A finished recording ends signed in; a stopped or an abandoned one has
/// not ended by the time the stand-in has said everything it recorded. An
/// error answer to the wait fails with the CLI's words, and a process that
/// ends with the wait unanswered fails with no reason.
// verifies: PROTO-CLAUDE-CODE-STREAM-R87
#[tokio::test]
async fn a_sign_in_ends_as_the_clis_answer_to_the_wait_says() {
    let (stand_in, result) = start(recorded(FINISHED)).await;
    let StartedSignIn { mut sign_in, .. } = started(result);
    assert_eq!(end(&mut sign_in).await, SignInEnd::SignedIn);
    // Asked again, it says the same.
    assert_eq!(end(&mut sign_in).await, SignInEnd::SignedIn);
    stop(&stand_in, sign_in).await;

    for name in [STOPPED, ABANDONED] {
        let (stand_in, result) = start(recorded(name)).await;
        let StartedSignIn { mut sign_in, .. } = started(result);
        stand_in.settled(0).await;
        assert_eq!(ended_yet(&mut sign_in).await, None, "{name} ended");
        stop(&stand_in, sign_in).await;
    }

    let script = recorded(FINISHED);
    let refusal = script.error_answer(WAIT, json!("The sign-in page expired."));
    let script = script.through_request(WAIT).then([Step::Emit(refusal)]);
    let (stand_in, result) = start(script).await;
    let StartedSignIn { mut sign_in, .. } = started(result);
    assert_eq!(
        end(&mut sign_in).await,
        SignInEnd::Failed(Some("The sign-in page expired.".into()))
    );
    stop(&stand_in, sign_in).await;

    // An error with no words of the CLI's has no reason to give.
    let script = recorded(FINISHED);
    let refusal = script.error_answer(WAIT, json!(""));
    let script = script.through_request(WAIT).then([Step::Emit(refusal)]);
    let (stand_in, result) = start(script).await;
    let StartedSignIn { mut sign_in, .. } = started(result);
    assert_eq!(end(&mut sign_in).await, SignInEnd::Failed(None));
    stop(&stand_in, sign_in).await;

    let script = recorded(FINISHED)
        .through_request(WAIT)
        .then([Step::Exit(1)]);
    let (stand_in, result) = start(script).await;
    let StartedSignIn { mut sign_in, .. } = started(result);
    assert_eq!(end(&mut sign_in).await, SignInEnd::Failed(None));
    sign_in.stop().await;
    assert_eq!(stand_in.running(), 0);
}

/// A transport on which the CLI's command is not found.
struct NoCommand;

#[async_trait]
impl Transport for NoCommand {
    async fn spawn(&self, _: ProcessSpec) -> io::Result<Box<dyn Process>> {
        Err(io::Error::new(io::ErrorKind::NotFound, "program not found"))
    }
}

/// Each way a start can fail, and what the adapter says of it.
fn failed_starts() -> Vec<(&'static str, Script, SignInStartError)> {
    let could_not = |s: &str| SignInStartError::CouldNotStart(s.to_string());
    let base = recorded(FINISHED);
    let refused_by = base.error_answer(AUTHENTICATE, json!("Your organization requires SSO."));
    let refused_silently = base.error_answer(AUTHENTICATE, Value::Null);
    let initialize_refused = base.error_answer(INITIALIZE, json!("bad hooks"));
    let address = |change: fn(&mut Value)| {
        recorded(FINISHED)
            .through_answer(AUTHENTICATE)
            .answering(AUTHENTICATE, |answer| change(&mut answer["response"]))
    };
    vec![
        (
            "claude_authenticate refused",
            recorded(FINISHED)
                .through_request(AUTHENTICATE)
                .then([Step::Emit(refused_by)]),
            SignInStartError::Refused("Your organization requires SSO.".into()),
        ),
        (
            "claude_authenticate refused with no words",
            recorded(FINISHED)
                .through_request(AUTHENTICATE)
                .then([Step::Emit(refused_silently)]),
            SignInStartError::Refused(String::new()),
        ),
        (
            "initialize refused",
            recorded(FINISHED)
                .through_request(INITIALIZE)
                .then([Step::Emit(initialize_refused)]),
            could_not(INITIALIZE_REFUSED),
        ),
        (
            "no automaticUrl",
            address(|answer| {
                answer.as_object_mut().unwrap().remove("automaticUrl");
            }),
            could_not(NO_ADDRESS),
        ),
        (
            "an empty automaticUrl",
            address(|answer| answer["automaticUrl"] = json!("")),
            could_not(NO_ADDRESS),
        ),
        (
            "an automaticUrl that is no string",
            address(|answer| answer["automaticUrl"] = json!(443)),
            could_not(NO_ADDRESS),
        ),
        (
            "the process ending before initialize is answered",
            recorded(FINISHED)
                .through_request(INITIALIZE)
                .then([Step::Exit(1)]),
            could_not(QUIT_EARLY),
        ),
        (
            "the process ending before claude_authenticate is answered",
            recorded(FINISHED)
                .through_request(AUTHENTICATE)
                .then([Step::Exit(1)]),
            could_not(QUIT_EARLY),
        ),
    ]
}

/// A start that does not get as far as the address fails, says which kind
/// of failure it was, and leaves no process running: the CLI's words when
/// it refused `claude_authenticate`, the adapter's own otherwise. Each
/// stand-in that does not exit by itself is held open, so only the adapter
/// can end it.
// verifies: PROTO-CLAUDE-CODE-STREAM-R86
#[tokio::test]
async fn a_start_that_fails_says_why_and_leaves_nothing_running() {
    for (case, script, expected) in failed_starts() {
        let (stand_in, result) = start(script).await;
        assert_eq!(failed(result), expected, "{case}");
        assert_eq!(stand_in.running(), 0, "{case}: a process is left running");
        assert!(
            stand_in.failures().is_empty(),
            "{case}: {:#?}",
            stand_in.failures()
        );
        // It gave up on the address without asking for the wait.
        assert!(
            !requests(&stand_in, 0).iter().any(|r| r.ends_with(WAIT)),
            "{case}"
        );
    }

    // A CLI whose command is not there, and one that is there and cannot
    // be run: the first in Core's words, the second in the adapter's.
    log();
    let adapter = ClaudeAdapter::new();
    let not_found = failed(adapter.start_sign_in(Arc::new(NoCommand)).await);
    assert_eq!(not_found, SignInStartError::NotFound);
    let no_script = StandIn::new([]);
    let would_not_run = failed(adapter.start_sign_in(Arc::new(no_script)).await);
    assert_eq!(
        would_not_run,
        SignInStartError::CouldNotStart(WOULD_NOT_RUN.into())
    );
}

/// The adapter's own words for a start it could not make say what went
/// wrong and never that the sign-in couldn't start, which the note has
/// said already: each is one sentence ending with a period.
// verifies: PROTO-CORE-API-R170
#[tokio::test]
async fn the_adapters_words_never_say_the_sign_in_did_not_start() {
    let mut sentences = Vec::new();
    for (case, script, _) in failed_starts() {
        let (_, result) = start(script).await;
        if let SignInStartError::CouldNotStart(sentence) = failed(result) {
            sentences.push((case, sentence));
        }
    }
    let no_script = StandIn::new([]);
    if let SignInStartError::CouldNotStart(sentence) = failed(
        ClaudeAdapter::new()
            .start_sign_in(Arc::new(no_script))
            .await,
    ) {
        sentences.push(("a command that cannot run", sentence));
    }
    assert_eq!(sentences.len(), 7, "{sentences:#?}");

    let saying_so = [
        "couldn't start",
        "can't start",
        "didn't start",
        "won't start",
        "could not start",
        "cannot start",
        "did not start",
        "couldn't be started",
        "wasn't started",
        "failed to start",
        "not started",
    ];
    for (case, sentence) in &sentences {
        let lower = sentence.to_lowercase();
        for words in saying_so {
            assert!(
                !lower.contains(words),
                "{case}: {sentence:?} says {words:?}"
            );
        }
        assert!(sentence.ends_with('.'), "{case}: {sentence:?}");
        assert_eq!(
            sentence.matches(". ").count(),
            0,
            "{case}: one sentence: {sentence:?}"
        );
        assert!(
            sentence.chars().next().is_some_and(char::is_uppercase),
            "{case}: {sentence:?}"
        );
    }
}

/// The stray lines the recordings show and some they don't: a line that is
/// not JSON, a `system` line, an answer to a request the adapter never
/// sent, and a second answer to one it did.
fn not_json() -> Step {
    Step::Emit("Claude Code is warming up".into())
}

fn system_line() -> Step {
    Step::Emit(json!({ "type": "system", "subtype": "status", "status": null }).to_string())
}

/// Lines that are not the first answer to one of the adapter's own
/// requests change nothing: before the start, a second `initialize` answer
/// that refuses and a second address leave the start as the first answers
/// made it; after it, nothing but the first answer to the wait ends the
/// sign-in, and a second answer, either way, changes nothing.
// verifies: PROTO-CLAUDE-CODE-STREAM-R88
#[tokio::test]
async fn lines_that_are_not_a_first_answer_change_nothing() {
    // Before the start.
    let base = recorded(FINISHED);
    let late_refusal = base.error_answer(INITIALIZE, json!("too late"));
    let second_address = base.answer(AUTHENTICATE, |answer| {
        answer["response"]["automaticUrl"] = json!("https://example.invalid/second");
    });
    let script = base
        .after_answer(
            INITIALIZE,
            [not_json(), system_line(), Step::Emit(late_refusal)],
        )
        .after_answer(AUTHENTICATE, [Step::Emit(second_address), not_json()]);
    let (stand_in, result) = start(script).await;
    let StartedSignIn { url, mut sign_in } = started(result);
    assert_eq!(url, Some(recorded_address(FINISHED, "automaticUrl")));
    assert_eq!(end(&mut sign_in).await, SignInEnd::SignedIn);
    assert!(stand_in.failures().is_empty(), "{:#?}", stand_in.failures());
    stop(&stand_in, sign_in).await;

    // After it: stray lines, then a gate, then the recorded ending and a
    // second, contrary answer to the wait.
    let base = recorded(FINISHED);
    let never_sent = json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": "ersa-claude_oauth_wait_for_completion-99",
            "response": {},
        }
    })
    .to_string();
    let second_authenticate = base.error_answer(AUTHENTICATE, json!("no longer"));
    let contrary = base.error_answer(WAIT, json!("expired after all"));
    let script = base
        .before_answer(
            WAIT,
            [
                not_json(),
                system_line(),
                Step::Emit(never_sent),
                Step::Emit(second_authenticate),
                Step::Gate,
            ],
        )
        .after_answer(WAIT, [Step::Emit(contrary)]);
    let (stand_in, result) = start(script).await;
    let StartedSignIn { mut sign_in, .. } = started(result);
    stand_in.settled(0).await;
    assert_eq!(ended_yet(&mut sign_in).await, None);
    stand_in.release(0);
    assert_eq!(end(&mut sign_in).await, SignInEnd::SignedIn);
    stand_in.settled(0).await;
    assert_eq!(end(&mut sign_in).await, SignInEnd::SignedIn);
    stop(&stand_in, sign_in).await;

    // A refusal first, and then a success, fails.
    let base = recorded(FINISHED);
    let refusal = base.error_answer(WAIT, json!("The sign-in page expired."));
    let success = base.answer(WAIT, |_| {});
    let script = base
        .through_request(WAIT)
        .then([Step::Emit(refusal), Step::Emit(success)]);
    let (stand_in, result) = start(script).await;
    let StartedSignIn { mut sign_in, .. } = started(result);
    stand_in.settled(0).await;
    assert_eq!(
        end(&mut sign_in).await,
        SignInEnd::Failed(Some("The sign-in page expired.".into()))
    );
    stop(&stand_in, sign_in).await;
}

// Markers in what a sign-in exchanges, looked for where nothing of it may
// go. Made up: no real address or account is written here.
const ADDRESS: &str = "ersa669addressmarker";
const MANUAL: &str = "ersa669manualmarker";
const ACCOUNT: &str = "ersa669accountmarker";
const STDERR: &str = "ersa669stderrmarker";
const INIT: &str = "ersa669initmarker";

/// The finished recording with markers in both addresses, in the account
/// the wait's answer names, and on stderr.
fn marked() -> Script {
    let account = json!({
        "email": format!("{ACCOUNT}@example.com"),
        "organization": format!("{ACCOUNT} org"),
        "subscriptionType": "Example Plan",
        "apiProvider": "firstParty",
    });
    recorded(FINISHED)
        .answering(AUTHENTICATE, |answer| {
            answer["response"]["automaticUrl"] = json!(format!(
                "https://claude.example/oauth/authorize?state={ADDRESS}"
            ));
            answer["response"]["manualUrl"] = json!(format!(
                "https://claude.example/oauth/authorize?state={MANUAL}"
            ));
        })
        .answering(WAIT, |answer| answer["response"]["account"] = account)
        .with_stderr([format!("debug: callback listening, state {STDERR}")])
}

/// Nothing a sign-in exchanges is written to the log, at any level, its
/// address and its stderr included, and the adapter hands Core nothing of
/// it but the address and a failure's reason: after a finished sign-in, a
/// failed one, and starts that failed, the log holds none of the markers,
/// and nothing returned does but the address itself.
// verifies: PROTO-CORE-API-R163
// verifies: PROTO-CLAUDE-CODE-STREAM-R90
#[tokio::test]
async fn nothing_a_sign_in_exchanges_is_logged_or_handed_on() {
    let log = log();
    let mut returned = Vec::new();
    let address = format!("https://claude.example/oauth/authorize?state={ADDRESS}");

    // Finished.
    let (stand_in, result) = start(marked()).await;
    let StartedSignIn { url, mut sign_in } = started(result);
    assert_eq!(url.as_deref(), Some(address.as_str()));
    returned.push(format!("{:?}", end(&mut sign_in).await));
    stop(&stand_in, sign_in).await;

    // Failed: the process ends with the wait unanswered.
    let script = marked().through_request(WAIT).then([Step::Exit(1)]);
    let (stand_in, result) = start(script).await;
    let StartedSignIn { url, mut sign_in } = started(result);
    assert_eq!(url.as_deref(), Some(address.as_str()));
    returned.push(format!("{:?}", end(&mut sign_in).await));
    sign_in.stop().await;
    assert_eq!(stand_in.running(), 0);

    // Starts that failed, saying something on stderr on the way.
    let refusal = marked().error_answer(INITIALIZE, json!(format!("bad hooks {INIT}")));
    for script in [
        marked()
            .through_request(INITIALIZE)
            .then([Step::Emit(refusal)]),
        marked().through_request(AUTHENTICATE).then([Step::Exit(1)]),
    ] {
        let (_, result) = start(script).await;
        returned.push(format!("{:?}", failed(result)));
    }

    for marker in [ADDRESS, MANUAL, ACCOUNT, STDERR, INIT] {
        assert!(!log.contains(marker), "the log holds {marker}");
        for value in &returned {
            assert!(!value.contains(marker), "{value} holds {marker}");
        }
    }
}
