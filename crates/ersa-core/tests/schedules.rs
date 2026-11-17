//! Two of Core's operations against each other, under every schedule
//! (ARCH-CORE, Concurrency): every order in which they can begin and in
//! which the calls they wait on (the backend's, git's) can complete.
//!
//! Each runs through Core's request handler, in process, on one thread
//! with tokio's clock paused (`support::schedule`). What is checked holds
//! whatever the order: each request is answered, with success or a refusal
//! the protocol names; what Core lists afterwards agrees with the answers;
//! and admission is left with nothing under way and nothing held.
//!
//! A failure prints the schedule it failed under; `ERSA_SCHEDULE` set to
//! that runs it alone.

mod support;

use std::collections::BTreeSet;
use std::io;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{Process, ProcessSpec, Transport};
use ersa_core::admission::{ProjectPhase, Run, WorkspacePhase};
use ersa_core::api::{self, ConnectionState};
use ersa_core::app_log::AppLog;
use ersa_core::persistence::Store;
use ersa_core::{Adapter, CommandTransport, Core};
use ersa_protocol::core_api::rpc::{Request, RequestId};
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, Filed, FiledRecord, Records, Stats};
use support::schedule::{Choices, Operation, Schedule, drive, every_schedule, replaying};
use support::{git_repo, kind, tempdir, test_config};

/// Core's command transport here: a command waits at the schedule's point,
/// then runs to its end on a blocking thread, which holds the paused clock
/// as the store's calls do, and its output is replayed.
struct OrderedCommands {
    schedule: Arc<Schedule>,
}

#[ersa_adapter::async_trait]
impl Transport for OrderedCommands {
    async fn spawn(&self, spec: ProcessSpec) -> io::Result<Box<dyn Process>> {
        self.schedule.point("a command").await;
        let run = spec.clone();
        let ran = tokio::task::spawn_blocking(move || {
            let mut command = std::process::Command::new(&run.program);
            command.args(&run.args).stdin(Stdio::null());
            if let Some(cwd) = &run.cwd {
                command.current_dir(cwd);
            }
            for name in &run.env_remove {
                command.env_remove(name);
            }
            command.envs(&run.env).output()
        });
        let output = ran.await.expect("the command's thread")?;
        let lines = |bytes: &[u8]| -> Vec<String> {
            let text = String::from_utf8_lossy(bytes);
            text.lines().map(str::to_string).collect()
        };
        FakeTransport::new(lines(&output.stdout))
            .with_stderr(lines(&output.stderr))
            .with_exit_code(output.status.code())
            .spawn(spec)
            .await
    }
}

/// A Core with one project, a fake backend and the schedule both wait on.
struct World {
    core: Arc<Core>,
    schedule: Arc<Schedule>,
    records: Records,
    filed: Filed,
    stats: Arc<Stats>,
    project: Value,
    _dir: tempfile::TempDir,
}

impl World {
    /// Built on real time, the schedule off: nothing waits yet.
    async fn new() -> World {
        let dir = tempdir();
        let schedule = Arc::new(Schedule::default());
        let mut adapter = FakeAdapter::new(1);
        adapter.schedule = Arc::clone(&schedule);
        let records = Arc::clone(&adapter.records);
        let filed = Arc::clone(&adapter.filed);
        let stats = Arc::clone(&adapter.stats);

        let mut config = test_config(&dir.path().join("data"), "never listened on");
        let commands = OrderedCommands {
            schedule: Arc::clone(&schedule),
        };
        config.command_transport = Some(CommandTransport(Arc::new(commands)));
        // The backends are detected once, in set-up, and not again while
        // the clock is the test's.
        config.failed_detection_ttl = Duration::from_secs(24 * 60 * 60);
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let store = Store::open(&config.db_path()).unwrap();
        // The app log writes on a thread of its own, which the paused clock
        // would not wait for: off.
        let app_log = AppLog::disabled(config.app_log_dir().join("off"));
        let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
        let core = Arc::new(Core::new(config, store, adapters, Arc::new(app_log)));
        // As a server does before it listens.
        let host = core.host_name().to_string();
        core.store.ensure_local_host(host).await.unwrap();
        core.admission.restore(core.store.existing().await.unwrap());
        let transport_for = |host: &str| core.session_transport_for(host);
        let no_facts = Default::default();
        core.sessions
            .recover(&no_facts, transport_for)
            .await
            .unwrap();

        let repo = dir.path().join("repo");
        git_repo(&repo);
        let path = repo.display().to_string();
        let add = json!({ "host_id": "local", "path": path, "trusted": false });
        let project = call(Arc::clone(&core), "project.add", add)
            .await
            .expect("the project is added");
        World {
            core,
            schedule,
            records,
            filed,
            stats,
            project,
            _dir: dir,
        }
    }

    async fn ok(&self, method: &'static str, params: Value) -> Value {
        let answer = call(Arc::clone(&self.core), method, params).await;
        answer.unwrap_or_else(|error| panic!("{method}: {error:#}"))
    }

    /// The request as an operation for the schedule to begin.
    fn request(&self, method: &'static str, params: Value) -> Operation<Answer> {
        (
            method,
            Box::pin(call(Arc::clone(&self.core), method, params)),
        )
    }

    /// The project's own folder as a workspace.
    fn root(&self) -> String {
        let state = self.core.admission.snapshot();
        let (id, _) = state.workspaces().find(|(_, w)| w.root).expect("a root");
        id.clone()
    }

    /// A session of the fake backend in `workspace` that has had one turn
    /// and is idle.
    async fn idle_session(&self, workspace: &str) -> String {
        let create = json!({ "workspace_id": workspace, "backend": fake::BACKEND });
        let session = self.ok("session.create", create).await;
        let id = session["id"].as_str().unwrap().to_string();
        // One message the backend records, and one that ends the turn.
        for text in ["hello", "idle"] {
            self.ok("session.send", json!({ "id": id, "text": text }))
                .await;
        }
        // Until its turn is over: waits on the session's state, on real
        // time, bounded so a session that never settles fails the test.
        for _ in 0..2_000 {
            let state = self.session(&id).await.expect("listed")["state"].clone();
            if state == "idle" {
                return id;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("session {id} never became idle");
    }

    async fn sessions(&self) -> Vec<Value> {
        let listed = self.ok("session.list", json!({})).await;
        listed.as_array().unwrap().clone()
    }

    async fn session(&self, id: &str) -> Option<Value> {
        let sessions = self.sessions().await;
        sessions.into_iter().find(|session| session["id"] == id)
    }

    /// Runs `operations` under `choices` on the paused clock, then lets
    /// what is left run on, and checks admission is at rest.
    async fn schedule(&self, choices: &Choices, operations: Vec<Operation<Answer>>) -> Vec<Answer> {
        tokio::time::pause();
        self.schedule.turn_on();
        let answers = drive(&self.schedule, choices, operations).await;
        self.schedule.turn_off();
        tokio::time::sleep(Duration::from_millis(1)).await;
        self.at_rest();
        answers
    }

    /// Admission has nothing under way but the runs of sessions that are
    /// running, and nothing held: every operation these requests began has
    /// ended there, whatever the order.
    fn at_rest(&self) {
        let state = self.core.admission.snapshot();
        assert!(state.held().is_empty(), "held: {state:#?}");
        assert!(!state.closing(), "{state:#?}");
        for (id, project) in state.projects() {
            assert_eq!(project.phase, ProjectPhase::Present, "{id}: {state:#?}");
        }
        for (id, workspace) in state.workspaces() {
            let ready = workspace.phase == WorkspacePhase::Ready;
            assert!(ready && workspace.deleting.is_none(), "{id}: {state:#?}");
        }
        for (id, session) in state.sessions() {
            let busy = session.exclusive.is_some() || session.send.is_some();
            let going = session.removing || session.forking || !session.stored();
            // A run is running or stopped, never left winding down.
            let winding_down = session.run == Run::Ending;
            assert!(!busy && !going && !winding_down, "{id}: {state:#?}");
        }
    }
}

/// What a request answered: its result, or its error.
type Answer = Result<Value, Value>;

/// One request, as a client of its own makes it.
async fn call(core: Arc<Core>, method: &'static str, params: Value) -> Answer {
    // Nothing reads what Core sends this client; room enough that Core
    // never waits to send.
    let (outbound, _unread) = tokio::sync::mpsc::channel(4096);
    let kick = Arc::new(tokio::sync::watch::Sender::new(false));
    let mut client = ConnectionState::new(outbound, kick);
    let hello = json!({ "client": "schedules", "client_version": "0", "api_versions": [6] });
    for (id, method, params) in [(0, "core.hello", hello), (1, method, params)] {
        let request = Request::new(RequestId::Number(id), method, params);
        let (response, _) = api::handle(&core, &mut client, request).await;
        let response = serde_json::to_value(&response).unwrap();
        match response.get("error") {
            Some(error) if id == 0 => panic!("core.hello: {error:#}"),
            Some(error) => return Err(error.clone()),
            None if id == 0 => {}
            None => return Ok(response["result"].clone()),
        }
    }
    unreachable!("the request was answered")
}

/// `answer`, in a word: `ok`, or its error's kind and its reason or detail.
fn outcome(answer: &Answer) -> String {
    match answer {
        Ok(_) => "ok".into(),
        Err(error) => {
            let data = &error["data"];
            match data["reason"].as_str().or(data["detail"].as_str()) {
                Some(why) => format!("{} ({why})", kind(error)),
                None => kind(error).to_string(),
            }
        }
    }
}

/// Core's blocking file-system jobs share one limit, Core-wide, and answer
/// `busy` past it: one scenario runs at a time, so none meets another's.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Runs `scenario` once under every schedule, each in a Core of its own,
/// and returns the outcomes seen, one line per pair of answers.
fn under_every_schedule<S, F>(name: &str, scenario: S) -> BTreeSet<String>
where
    S: Fn(Choices) -> F,
    F: std::future::Future<Output = String>,
{
    let _alone = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut seen = BTreeSet::new();
    let schedules = every_schedule(name, |choices| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcomes = runtime.block_on(scenario(choices.clone()));
        eprintln!("  {}: {} => {outcomes}", choices.seed(), choices.steps());
        seen.insert(outcomes);
    });
    eprintln!("{name}: {schedules} schedules, outcomes {seen:#?}");
    // One schedule run alone says nothing of how many there are.
    assert!(replaying() || schedules > 3, "only {schedules} schedules");
    seen
}

/// Every one of `wanted` happened under some schedule.
fn all_happened(seen: &BTreeSet<String>, wanted: &[&str]) {
    if replaying() {
        return;
    }
    for outcomes in wanted {
        assert!(
            seen.contains(*outcomes),
            "{outcomes} never happened: {seen:#?}"
        );
    }
}

/// The driver itself: two operations that each wait on one call run under
/// every order of their beginnings and completions, once each, and a
/// schedule replayed from what a failure would print takes the same steps.
#[test]
fn every_order_of_two_operations_runs_once_and_replays_from_its_seed() {
    fn run(choices: Choices) -> String {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .unwrap();
        runtime.block_on(async {
            let schedule = Arc::new(Schedule::default());
            schedule.turn_on();
            let waits = |name: &'static str, call: &'static str| -> Operation<()> {
                let schedule = Arc::clone(&schedule);
                (name, Box::pin(async move { schedule.point(call).await }))
            };
            let operations = vec![waits("one", "one's call"), waits("two", "two's call")];
            drive(&schedule, &choices, operations).await;
        });
        choices.steps()
    }
    let mut orders = Vec::new();
    let count = every_schedule("two operations", |choices| {
        let steps = run(choices.clone());
        orders.push((choices.seed(), steps));
    });
    if replaying() {
        return;
    }
    // Each begins before its call is let go: six orders of the four steps.
    assert_eq!(count, 6);
    let distinct: BTreeSet<&String> = orders.iter().map(|(_, steps)| steps).collect();
    assert_eq!(distinct.len(), 6, "{orders:#?}");
    let last = "begin two, let go two's call, begin one, let go one's call";
    assert!(orders.iter().any(|(_, steps)| steps == last), "{orders:#?}");
    for (seed, steps) in &orders {
        assert_eq!(&run(Choices::replay(seed)), steps, "{seed}");
    }
}

#[test]
fn a_delete_against_a_create_in_the_same_workspace() {
    let seen = under_every_schedule("delete against create", |choices| async move {
        let world = World::new().await;
        let workspace = world.root();
        let first = world.idle_session(&workspace).await;
        // In the project's own folder a delete takes no files with it, so
        // it needs no token for them.
        let delete = json!({ "id": first });
        let create = json!({ "workspace_id": workspace, "backend": fake::BACKEND });

        let operations = vec![
            world.request("session.delete", delete),
            world.request("session.create", create),
        ];
        let answers = world.schedule(&choices, operations).await;
        let (deleted, created) = (&answers[0], &answers[1]);

        // The delete goes through, whatever the create does; the create is
        // let in, or told the workspace is busy with the delete.
        assert_eq!(outcome(deleted), "ok");
        let created_outcome = outcome(created);
        let let_in_or_busy = ["ok", "invalid_state (busy)"];
        assert!(
            let_in_or_busy.contains(&created_outcome.as_str()),
            "{created:#?}"
        );
        // What Core lists agrees: the deleted session is gone with its
        // record, and the new one is there only when its create said so.
        let listed = world.sessions().await;
        assert!(listed.iter().all(|session| session["id"] != first.as_str()));
        match created {
            Ok(session) => {
                assert_eq!(listed.len(), 1, "{listed:#?}");
                assert_eq!(listed[0]["id"], session["id"]);
            }
            Err(_) => assert!(listed.is_empty(), "{listed:#?}"),
        }
        assert_eq!(world.records.lock().unwrap().len(), listed.len());
        format!("delete {}, create {created_outcome}", outcome(deleted))
    });
    // Both ways round happened: the create let in, and held off.
    let both = [
        "delete ok, create ok",
        "delete ok, create invalid_state (busy)",
    ];
    all_happened(&seen, &both);
}

/// A request that starts a found session's backend (`method`) against
/// the removal of the session's workspace.
fn starting_a_found_session_against_the_removal_of_its_workspace(
    method: &'static str,
    extra: Value,
) -> BTreeSet<String> {
    under_every_schedule(method, move |choices| {
        let extra = extra.clone();
        async move {
            let world = World::new().await;
            // A session started elsewhere, found in an isolated workspace.
            let create = json!({ "project_id": world.project["id"] });
            let isolated = world.ok("workspace.create", create).await;
            let path = isolated["path"].as_str().unwrap().to_string();
            let record = vec![fake::recorded_user("m-1", "found")];
            {
                let mut records = world.records.lock().unwrap();
                records.insert("rec-found".into(), record);
                let (under, cwd) = (path.clone(), path);
                let filed = FiledRecord { under, cwd };
                world
                    .filed
                    .lock()
                    .unwrap()
                    .insert("rec-found".into(), filed);
            }
            let found = world.sessions().await.remove(0);
            let mut params = extra;
            params["id"] = found["id"].clone();

            let operations = vec![
                world.request(method, params),
                world.request("workspace.remove", json!({ "id": isolated["id"] })),
            ];
            let answers = world.schedule(&choices, operations).await;
            let (started, removed) = (&answers[0], &answers[1]);
            let outcomes = format!("start {}, removal {}", outcome(started), outcome(removed));

            // Exactly one of them goes through: no backend is left running
            // in a folder that is gone, and no removal goes through around
            // one that runs.
            let listed = world.sessions().await;
            let workspaces = json!({ "project_id": world.project["id"] });
            let workspaces = world.ok("workspace.list", workspaces).await;
            let workspaces = workspaces.as_array().unwrap();
            let still_there = workspaces.iter().any(|w| w["id"] == isolated["id"]);
            let starts = world.stats.starts.load(Ordering::SeqCst);
            match (started, removed) {
                (Ok(_), Err(refused)) => {
                    assert_eq!(outcome(removed), "invalid_state", "{refused:#}");
                    assert!(still_there);
                    assert_eq!(listed.len(), 1, "{listed:#?}");
                    assert_eq!(starts, 1);
                }
                (Err(refused), Ok(_)) => {
                    // Refused as busy while the removal holds the
                    // workspace, as while a delete does; gone after. No
                    // backend was started in the folder about to go.
                    match kind(refused) {
                        "invalid_state" => assert_eq!(refused["data"]["detail"], "busy"),
                        gone => assert_eq!(gone, "not_found", "{refused:#}"),
                    }
                    assert!(!still_there);
                    assert!(listed.is_empty(), "{listed:#?}");
                    assert_eq!(starts, 0);
                }
                (Ok(_), Ok(_)) | (Err(_), Err(_)) => panic!("{outcomes}: {answers:#?}"),
            }
            // Its record is the backend's, and stays either way.
            assert!(world.records.lock().unwrap().contains_key("rec-found"));
            outcomes
        }
    })
}

// verifies: FLOW-RESUME-SESSION-R13
// verifies: FLOW-RESUME-SESSION-R14
#[test]
fn a_resume_against_the_removal_of_its_workspace() {
    let seen =
        starting_a_found_session_against_the_removal_of_its_workspace("session.resume", json!({}));
    all_happened(
        &seen,
        &[
            "start ok, removal invalid_state",
            "start invalid_state (busy), removal ok",
            "start not_found, removal ok",
        ],
    );
}

// verifies: FLOW-RESUME-SESSION-R13
// verifies: FLOW-RESUME-SESSION-R14
#[test]
fn a_message_to_a_found_session_against_the_removal_of_its_workspace() {
    let seen = starting_a_found_session_against_the_removal_of_its_workspace(
        "session.send",
        json!({ "text": "hello" }),
    );
    all_happened(
        &seen,
        &[
            "start ok, removal invalid_state",
            "start invalid_state (busy), removal ok",
            "start not_found, removal ok",
        ],
    );
}

// verifies: PROTO-CORE-API-R77
// verifies: PROTO-CORE-API-R85
#[test]
fn an_archive_against_a_send() {
    let seen = under_every_schedule("archive against send", |choices| async move {
        let world = World::new().await;
        let session = world.idle_session(&world.root()).await;
        let stops = || world.stats.stops.load(Ordering::SeqCst);
        let starts = world.stats.starts.load(Ordering::SeqCst);

        // The message is one the backend answers and ends its turn on.
        let archive = json!({ "id": session, "archived": true });
        let send = json!({ "id": session, "text": "finish" });
        let operations = vec![
            world.request("session.set_archived", archive),
            world.request("session.send", send),
        ];
        let answers = world.schedule(&choices, operations).await;
        let (archived, sent) = (&answers[0], &answers[1]);
        let outcomes = format!("archive {}, send {}", outcome(archived), outcome(sent));
        assert!(archived.is_ok() || sent.is_ok(), "{outcomes}");
        let row = world.session(&session).await.expect("still listed");

        match archived {
            // An archived session is stopped.
            Ok(answer) => {
                assert_eq!(answer["archived"], true, "{outcomes}");
                assert_eq!(row["archived"], true, "{outcomes}: {row:#}");
                assert_eq!(row["state"], "ended", "{outcomes}: {row:#}");
            }
            // Refused while the message is being sent, and nothing changed:
            // not archived, not stopped (PROTO-CORE-API-R77).
            Err(refused) => {
                assert_eq!(kind(refused), "invalid_state", "{refused:#}");
                assert_eq!(refused["data"]["reason"], "session_busy", "{refused:#}");
                assert_eq!(row["archived"], false, "{outcomes}: {row:#}");
                assert_eq!(row["state"], "idle", "{outcomes}: {row:#}");
                assert_eq!(stops(), 0, "{outcomes}");
            }
        }
        if let Err(refused) = sent {
            assert_eq!(kind(refused), "invalid_state", "{refused:#}");
            match refused["data"]["reason"].as_str() {
                // After the archive: the session is archived.
                Some(reason) => assert_eq!(reason, "session_archived", "{refused:#}"),
                // While the archive is under way (PROTO-CORE-API-R85).
                None => {
                    let message = refused["message"].as_str().unwrap();
                    assert!(message.contains("being archived"), "{refused:#}");
                }
            }
        }
        // A refused send starts no turn and resumes nothing; one that went
        // through reached the backend once.
        let messages = world.stats.messages.lock().unwrap();
        let taken = messages.iter().filter(|m| m.text == "finish").count();
        drop(messages);
        assert_eq!(taken, usize::from(sent.is_ok()), "{outcomes}");
        assert_eq!(
            world.stats.starts.load(Ordering::SeqCst),
            starts,
            "{outcomes}"
        );
        outcomes
    });
    all_happened(
        &seen,
        &[
            "archive ok, send ok",
            "archive ok, send invalid_state",
            "archive ok, send invalid_state (session_archived)",
            "archive invalid_state (session_busy), send ok",
        ],
    );
}
