//! Closing and quitting (FLOW-CLOSE-AND-QUIT) against real processes: Quit
//! stops a real `ersa-core` and the backend of a running session, a Core
//! that will not stop is ended, a Shell that dies leaves Core serving, and a
//! second launch finds the first.
//!
//! Needs the `ersa-core` executable, as tests/bridge.rs does. Each test's
//! Core serves an endpoint of its own, so the user's own Core is never
//! reached or stopped.
//!
//! Some processes these tests need are this test binary run again, with
//! `ERSA_QUIT_TEST_HELPER` naming the part to play (the `helper_*` tests,
//! ignored unless asked for, and inert without the variable).

mod common;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use common::{
    CLEANUP, OPEN, finish, git_repo, open, private_location, request, stand_in_backend,
    unique_label,
};
use ersa_shell::bridge::{self, LinkEvent};
use ersa_shell::instance::{self, Instance};
use ersa_shell::launch::{self, CoreLocation, How};
use ersa_shell::process::CoreProcess;
use ersa_shell::quit::{self, QUIT_LIMIT, Stopped};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// Which part a run of this binary plays, and where it reports.
const HELPER: &str = "ERSA_QUIT_TEST_HELPER";
const HELPER_ENDPOINT: &str = "ERSA_QUIT_TEST_ENDPOINT";
const HELPER_PIDS: &str = "ERSA_QUIT_TEST_PIDS";

// verifies: FLOW-CLOSE-AND-QUIT-R03
#[tokio::test]
async fn quit_shuts_core_down_and_returns_once_it_has_exited() {
    let dir = tempfile::tempdir().expect("temp dir");
    let location = private_location(dir.path());
    let checks = tokio::spawn({
        let location = location.clone();
        async move {
            let (_link, how, _events) = open(&location).await;
            assert_eq!(how, How::Launched);
            let stopped = quit::shut_down(&location.endpoint, QUIT_LIMIT, None)
                .await
                .expect("quit");
            let Stopped::Exited { pid } = stopped else {
                panic!("Core should stop when asked, got {stopped:?}");
            };
            assert!(!running(pid), "Core's process {pid} outlived the quit");
            assert!(!bridge::alive(&location.endpoint).await);
        }
    });
    finish(checks, &location).await;
}

// verifies: FLOW-CLOSE-AND-QUIT-R05
#[tokio::test]
async fn a_session_running_at_quit_ends_user_stopped_and_leaves_no_process() {
    let dir = tempfile::tempdir().expect("temp dir");
    let pids_file = dir.path().join("stand-in.pids");
    let mut location = private_location(dir.path());
    location.env = vec![
        ("ERSA_CLAUDE".into(), stand_in_backend().into_os_string()),
        (
            "ERSA_STAND_IN_PIDS".into(),
            pids_file.clone().into_os_string(),
        ),
        // Nothing the session does may read the user's own Claude home.
        (
            "CLAUDE_CONFIG_DIR".into(),
            dir.path().join("claude").into_os_string(),
        ),
    ];
    let repo = dir.path().join("repo");
    git_repo(&repo);

    let checks = tokio::spawn({
        let location = location.clone();
        async move {
            let (link, _how, events) = open(&location).await;
            let host = link.hello()["host"]["id"].clone();
            let project = request(
                &link,
                "project.add",
                json!({ "host_id": host, "path": repo.display().to_string(), "trusted": false }),
            )
            .await;
            let workspace = request(
                &link,
                "workspace.create",
                json!({ "project_id": project["id"] }),
            )
            .await;
            let session = request(
                &link,
                "session.create",
                json!({ "workspace_id": workspace["id"], "backend": "claude-code" }),
            )
            .await;
            let id = session["id"].as_str().expect("a session id").to_string();
            request(
                &link,
                "session.subscribe",
                json!({ "id": id, "from_seq": 0 }),
            )
            .await;
            let (backend, grandchild) = read_pids(&pids_file).await;
            assert!(running(backend) && running(grandchild));

            let stopped = quit::shut_down(&location.endpoint, QUIT_LIMIT, None)
                .await
                .expect("quit");
            assert!(
                matches!(stopped, Stopped::Exited { .. }),
                "Core should stop when asked, got {stopped:?}"
            );

            eventually_gone(backend).await;
            eventually_gone(grandchild).await;
            drop((link, events));

            // Core closes its connections before it stops the sessions, so
            // how the session ended is read from the next Core's record.
            let (link, how, mut events) = open(&location).await;
            assert_eq!(how, How::Launched);
            request(
                &link,
                "session.subscribe",
                json!({ "id": id, "from_seq": 0 }),
            )
            .await;
            let mut reason = None;
            while let Ok(Some(event)) = tokio::time::timeout(CLEANUP, events.recv()).await {
                if let LinkEvent::Notification { method, params } = event
                    && method == "session.event"
                    && params["event"]["event"]["type"] == "session.ended"
                {
                    reason = Some(params["event"]["event"]["reason"].clone());
                    break;
                }
            }
            assert_eq!(reason, Some(json!("user_stopped")));
        }
    });
    finish(checks, &location).await;
}

// verifies: FLOW-CLOSE-AND-QUIT-R06
#[tokio::test]
async fn a_core_that_does_not_stop_in_time_is_ended_with_what_it_started() {
    assert_eq!(QUIT_LIMIT, Duration::from_secs(10));
    let dir = tempfile::tempdir().expect("temp dir");
    let label = unique_label("shell-wedged");
    let endpoint = ersa_core::platform::labelled_endpoint(&label).expect("an endpoint");
    let pids_file = dir.path().join("wedged.pids");
    common::end_with_this_run();
    let mut wedged = helper("wedged_core")
        .env(HELPER_ENDPOINT, &endpoint)
        .env(HELPER_PIDS, &pids_file)
        .spawn()
        .expect("start the wedged Core");
    let Some((core, child)) = try_read_pids(&pids_file).await else {
        let _ = wedged.kill();
        let _ = wedged.wait();
        panic!("the wedged Core did not start");
    };
    let outcome = tokio::spawn({
        let endpoint = endpoint.clone();
        async move {
            let limit = Duration::from_secs(2);
            // A client took the one pipe instance the wedged Core had
            // waiting, as the bridge's liveness probe does; on Windows the
            // endpoint can no longer say which process serves it.
            let _probe = hold_a_client(&endpoint);
            if cfg!(windows) {
                let lost = quit::shut_down(&endpoint, limit, None).await;
                assert!(lost.is_err(), "found a Core nothing can reach: {lost:?}");
            }
            // The Shell that launched it still holds it.
            let launched = CoreProcess::open(core).expect("hold the Core");
            let started = std::time::Instant::now();
            let stopped = quit::shut_down(&endpoint, limit, Some(launched))
                .await
                .expect("quit");
            assert_eq!(stopped, Stopped::Killed { pid: core });
            assert!(
                started.elapsed() >= limit,
                "ended before the limit: {:?}",
                started.elapsed()
            );
            assert!(!running(core));
            eventually_gone(child).await;
        }
    })
    .await;
    // However the checks ended, nothing the wedged Core started outlives them.
    let _ = wedged.kill();
    let _ = wedged.wait();
    kill(child);
    if let Err(failed) = outcome {
        std::panic::resume_unwind(failed.into_panic());
    }
}

// verifies: FLOW-CLOSE-AND-QUIT-R07
#[tokio::test]
async fn a_shell_that_exits_without_quit_leaves_core_serving() {
    let dir = tempfile::tempdir().expect("temp dir");
    let location = private_location(dir.path());
    // A Shell that launches Core and then dies, as a crash would end it.
    let mut shell = helper("shell_then_crash")
        .env(HELPER_ENDPOINT, &location.endpoint)
        .env("ERSA_QUIT_TEST_LOCATION", location_json(&location))
        .stdout(Stdio::piped())
        .spawn()
        .expect("start the Shell");
    let ready = wait_for_line(&mut shell, "launched");
    let _ = shell.kill();
    let _ = shell.wait();

    let checks = tokio::spawn({
        let location = location.clone();
        async move {
            assert!(ready, "the Shell did not launch Core");
            if !bridge::alive(&location.endpoint).await {
                // Why: the connect error, and what Core itself wrote.
                let connect = match ersa_core::connect(&location.endpoint).await {
                    Ok(_) => "connects".to_string(),
                    Err(e) => format!("{e} ({:?})", e.kind()),
                };
                let log = location
                    .log_file
                    .as_deref()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .unwrap_or_default();
                panic!("Core stopped with the Shell: {connect}; Core's log:\n{log}");
            }
            let (_link, how, _events) = open(&location).await;
            assert_eq!(how, How::Attached, "the next launch attaches to that Core");
        }
    });
    finish(checks, &location).await;
}

// verifies: FLOW-CLOSE-AND-QUIT-R02
#[tokio::test]
async fn a_second_launch_shows_the_first_window_instead_of_opening_one() {
    let label = instance::label_for(Some(&unique_label("t")));
    let endpoint = ersa_core::platform::labelled_endpoint(&label).expect("an endpoint");
    let Instance::First(listener) = instance::claim(&endpoint).await.expect("claim") else {
        panic!("nothing else serves {endpoint}");
    };
    let (shown, mut shows) = mpsc::unbounded_channel();
    let first = tokio::spawn(instance::serve_requests(listener, move || {
        let _ = shown.send(());
    }));
    // Every later launch finds the first and asks it, not only the next one:
    // a launch that lets go at once must not cost the first its endpoint.
    for launch in 0..3 {
        let later = instance::claim(&endpoint).await.expect("a later claim");
        assert!(matches!(later, Instance::Running), "launch {launch}");
        tokio::time::timeout(Duration::from_secs(10), shows.recv())
            .await
            .expect("the first Shell is asked to show its window");
    }
    first.abort();
}

#[test]
fn a_window_attached_by_label_has_its_own_instance() {
    assert_eq!(instance::label_for(None), "window");
    assert_eq!(instance::label_for(Some("replay")), "window-replay");
}

/// A Core that never accepts a connection, with a child of its own; writes
/// `<own pid> <child pid>`.
#[tokio::test]
#[ignore = "a helper process for a_core_that_does_not_stop_in_time_is_ended_with_what_it_started"]
async fn helper_wedged_core() {
    if std::env::var(HELPER).as_deref() != Ok("wedged_core") {
        return;
    }
    let endpoint = std::env::var(HELPER_ENDPOINT).expect("an endpoint");
    let listener = ersa_core::platform::Listener::bind(&endpoint)
        .await
        .expect("bind");
    // Never waited on: the quit under test ends it, and this process with it.
    #[allow(clippy::zombie_processes)]
    // It sleeps on its own, so only ending the tree ends it; on Windows the
    // test run's job ends it if the test does not.
    let child = Command::new(stand_in_backend())
        .env("ERSA_STAND_IN_SLEEP", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("start a child");
    write_pids(std::process::id(), child.id());
    // Wedged before it ever accepts: whoever connects takes the one waiting
    // pipe instance, and nobody else gets in.
    let _listener = listener;
    tokio::time::sleep(Duration::from_secs(600)).await;
}

/// A Shell that launches Core, says `launched`, and waits to be killed.
#[tokio::test]
#[ignore = "a helper process for a_shell_that_exits_without_quit_leaves_core_serving"]
async fn helper_shell_then_crash() {
    if std::env::var(HELPER).as_deref() != Ok("shell_then_crash") {
        return;
    }
    let location =
        location_from_json(&std::env::var("ERSA_QUIT_TEST_LOCATION").expect("a location"));
    let (_stream, how) = launch::attach_or_launch(&location).await.expect("launch");
    assert_eq!(how, How::Launched);
    println!("launched");
    tokio::time::sleep(Duration::from_secs(600)).await;
}

/// This test binary, run again to play `part`. On Unix it leads a process
/// group of its own, as a Core the Shell launched does.
fn helper(part: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("this test binary"));
    command
        .args([
            &format!("helper_{part}"),
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env(HELPER, part)
        .stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

fn write_pids(first: u32, second: u32) {
    let path = PathBuf::from(std::env::var_os(HELPER_PIDS).expect("a pids file"));
    let partial = path.with_extension("partial");
    std::fs::write(&partial, format!("{first} {second}")).expect("write the pids");
    std::fs::rename(&partial, &path).expect("publish the pids");
}

/// The two process ids in `path`, once something has written them.
async fn read_pids(path: &Path) -> (u32, u32) {
    try_read_pids(path)
        .await
        .unwrap_or_else(|| panic!("nothing wrote {}", path.display()))
}

async fn try_read_pids(path: &Path) -> Option<(u32, u32)> {
    let read = async {
        loop {
            if let Ok(text) = std::fs::read_to_string(path) {
                let ids: Vec<u32> = text
                    .split_whitespace()
                    .map(|id| id.parse().expect("a process id"))
                    .collect();
                return (ids[0], ids[1]);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(OPEN, read).await.ok()
}

/// Reads `child`'s output until a line equals `line`; false if it ends
/// first, or does not come within [`OPEN`].
fn wait_for_line(child: &mut Child, line: &str) -> bool {
    let stdout = child.stdout.take().expect("piped stdout");
    let line = line.to_string();
    let (found, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let seen = BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .any(|l| l.trim() == line);
        let _ = found.send(seen);
    });
    result.recv_timeout(OPEN).unwrap_or(false)
}

/// What a helper needs of a location, passed in the environment.
fn location_json(location: &CoreLocation) -> String {
    let os = |s: &std::ffi::OsStr| s.to_string_lossy().into_owned();
    json!({
        "program": location.program,
        "endpoint": location.endpoint,
        "serve_args": location.serve_args.iter().map(|a| os(a)).collect::<Vec<_>>(),
        "log_file": location.log_file,
    })
    .to_string()
}

fn location_from_json(text: &str) -> CoreLocation {
    let value: Value = serde_json::from_str(text).expect("a location");
    let string = |v: &Value| v.as_str().expect("a string").to_string();
    CoreLocation {
        program: PathBuf::from(string(&value["program"])),
        endpoint: string(&value["endpoint"]),
        serve_args: value["serve_args"]
            .as_array()
            .expect("arguments")
            .iter()
            .map(|a| string(a).into())
            .collect(),
        env: Vec::new(),
        log_file: value["log_file"].as_str().map(PathBuf::from),
        start_timeout: Duration::from_secs(60),
        launch: true,
    }
}

/// Whether process `pid` is still running.
fn running(pid: u32) -> bool {
    if cfg!(windows) {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .expect("tasklist runs");
        // A found process is listed as `"name","<pid>",...`; the message for
        // none is localised, so look for the quoted id instead.
        String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
    } else {
        // Gone, or a zombie waiting to be reaped, is not running.
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => !stat
                .rsplit(')')
                .next()
                .is_some_and(|rest| rest.trim_start().starts_with('Z')),
            Err(_) => false,
        }
    }
}

/// A client connection to `endpoint` that sends nothing, held until dropped.
fn hold_a_client(endpoint: &str) -> Box<dyn std::any::Any + Send> {
    #[cfg(windows)]
    let client: Box<dyn std::any::Any + Send> = Box::new(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(endpoint)
            .expect("a client"),
    );
    #[cfg(unix)]
    let client: Box<dyn std::any::Any + Send> =
        Box::new(std::os::unix::net::UnixStream::connect(endpoint).expect("a client"));
    client
}

/// Ends process `pid` if it still runs; for cleaning up after a failed test.
fn kill(pid: u32) {
    if !running(pid) {
        return;
    }
    let _ = if cfg!(windows) {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
    } else {
        Command::new("kill").args(["-9", &pid.to_string()]).output()
    };
}

/// Waits up to ten seconds for process `pid` to stop running.
async fn eventually_gone(pid: u32) {
    for _ in 0..100 {
        if !running(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("process {pid} is still running");
}
