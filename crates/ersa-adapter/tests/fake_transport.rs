//! The `test-support` fake transport behaves like a process.

use std::time::Duration;

use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{ProcessExit, ProcessSpec, Transport};

fn spec(program: &str) -> ProcessSpec {
    ProcessSpec {
        program: program.into(),
        args: vec!["--flag".into()],
        ..ProcessSpec::default()
    }
}

#[tokio::test]
async fn replays_stdout_and_stderr_then_exits_with_the_scripted_code() {
    let transport = FakeTransport::new(["one", "two"])
        .with_stderr(["warning"])
        .with_exit_code(2);
    let mut process = transport.spawn(spec("backend")).await.expect("spawn");
    let mut stdout = process.take_stdout().expect("stdout");
    let mut stderr = process.take_stderr().expect("stderr");

    assert_eq!(stdout.next_line().await.unwrap().as_deref(), Some("one"));
    assert_eq!(stdout.next_line().await.unwrap().as_deref(), Some("two"));
    assert_eq!(stdout.next_line().await.unwrap(), None);
    assert_eq!(
        stderr.next_line().await.unwrap().as_deref(),
        Some("warning")
    );
    assert_eq!(stderr.next_line().await.unwrap(), None);
    assert_eq!(process.wait().await.unwrap(), ProcessExit { code: Some(2) });

    assert_eq!(transport.spawned(), [spec("backend")]);
}

#[tokio::test]
async fn records_stdin_across_clones() {
    let transport = FakeTransport::new(Vec::<String>::new());
    let handed_out = transport.clone();
    let mut process = handed_out.spawn(spec("backend")).await.expect("spawn");
    let mut stdin = process.take_stdin().expect("stdin");

    stdin.write_line("first").await.unwrap();
    stdin.write_line("second").await.unwrap();
    assert!(stdin.write_line("bad\nline").await.is_err());

    assert_eq!(transport.stdin_lines(), ["first", "second"]);
}

#[tokio::test]
async fn hold_open_keeps_stdout_open_until_killed() {
    let transport = FakeTransport::new(["ready"]).hold_open();
    let mut process = transport.spawn(spec("backend")).await.expect("spawn");
    let mut stdin = process.take_stdin().expect("stdin");
    let mut stdout = process.take_stdout().expect("stdout");

    assert_eq!(stdout.next_line().await.unwrap().as_deref(), Some("ready"));
    // Past the script the read waits, like a backend waiting for input; and
    // being cancelled by the timeout does not end the stream.
    let pending = tokio::time::timeout(Duration::from_millis(20), stdout.next_line()).await;
    assert!(pending.is_err());

    process.kill().await.unwrap();
    assert_eq!(stdout.next_line().await.unwrap(), None);
    assert_eq!(process.wait().await.unwrap(), ProcessExit { code: None });
    assert_eq!(
        stdin.write_line("too late").await.unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
}

#[tokio::test]
async fn hold_open_ends_when_stdin_closes() {
    let transport = FakeTransport::new(Vec::<String>::new()).hold_open();
    let mut process = transport.spawn(spec("backend")).await.expect("spawn");
    let mut stdout = process.take_stdout().expect("stdout");

    drop(process.take_stdin());
    assert_eq!(stdout.next_line().await.unwrap(), None);
    assert!(process.wait().await.unwrap().success());
}

#[tokio::test]
async fn wait_for_stdin_pauses_stdout_until_a_line_arrives_and_checks_it() {
    let transport = FakeTransport::new(["ready"])
        .wait_for_stdin(|line| {
            if line == "ping" {
                Ok(())
            } else {
                Err(format!("expected ping, got {line}"))
            }
        })
        .then_stdout(["pong"])
        .wait_for_stdin(|_| Err("no more input expected".into()))
        .then_stdout(["never sent"]);
    let mut process = transport.spawn(spec("backend")).await.expect("spawn");
    let mut stdin = process.take_stdin().expect("stdin");
    let mut stdout = process.take_stdout().expect("stdout");

    assert_eq!(stdout.next_line().await.unwrap().as_deref(), Some("ready"));
    // Paused until stdin gets a line; a cancelled read loses nothing.
    let pending = tokio::time::timeout(Duration::from_millis(20), stdout.next_line()).await;
    assert!(pending.is_err());

    stdin.write_line("ping").await.unwrap();
    assert_eq!(stdout.next_line().await.unwrap().as_deref(), Some("pong"));

    // A line the check rejects ends the script and is reported.
    stdin.write_line("surprise").await.unwrap();
    assert_eq!(stdout.next_line().await.unwrap(), None);
    assert_eq!(transport.stub_failures(), ["no more input expected"]);
}
