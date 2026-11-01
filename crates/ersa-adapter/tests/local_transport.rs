//! `LocalTransport` against real processes.
//!
//! The helper programs differ per OS, chosen with `cfg!(windows)`, which is an
//! ordinary boolean expression rather than conditional compilation: both
//! branches compile everywhere, and only test code makes the choice.

use std::time::Duration;

use ersa_adapter::{LocalTransport, Process, ProcessSpec, Transport};

/// A spec that runs `command` in the platform's shell.
fn shell(command: &str) -> ProcessSpec {
    let (program, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    ProcessSpec {
        program: program.into(),
        args: vec![flag.into(), command.into()],
        ..ProcessSpec::default()
    }
}

/// A process that copies its stdin to its stdout, line by line, until stdin
/// closes. `findstr` with the pattern `^` matches every line.
fn echo() -> ProcessSpec {
    if cfg!(windows) {
        ProcessSpec {
            program: "findstr".into(),
            args: vec!["^".into()],
            ..ProcessSpec::default()
        }
    } else {
        ProcessSpec {
            program: "cat".into(),
            ..ProcessSpec::default()
        }
    }
}

async fn spawn(spec: ProcessSpec) -> Box<dyn Process> {
    LocalTransport.spawn(spec).await.expect("spawn")
}

#[tokio::test]
async fn lines_written_to_stdin_come_back_on_stdout() {
    let mut process = spawn(echo()).await;
    let mut stdin = process.take_stdin().expect("stdin");
    let mut stdout = process.take_stdout().expect("stdout");

    stdin.write_line(r#"{"type":"user"}"#).await.expect("write");
    stdin.write_line("second line").await.expect("write");
    // Dropping the writer closes stdin, which ends the echo.
    drop(stdin);

    assert_eq!(
        stdout.next_line().await.expect("read").as_deref(),
        Some(r#"{"type":"user"}"#)
    );
    assert_eq!(
        stdout.next_line().await.expect("read").as_deref(),
        Some("second line")
    );
    assert_eq!(stdout.next_line().await.expect("read"), None);
    assert!(process.wait().await.expect("wait").success());
}

#[tokio::test]
async fn each_stream_is_taken_once() {
    let mut process = spawn(echo()).await;
    assert!(process.take_stdin().is_some());
    assert!(process.take_stdin().is_none());
    assert!(process.take_stdout().is_some());
    assert!(process.take_stdout().is_none());
    assert!(process.take_stderr().is_some());
    assert!(process.take_stderr().is_none());
}

#[tokio::test]
async fn a_cancelled_read_loses_nothing() {
    let mut process = spawn(echo()).await;
    let mut stdin = process.take_stdin().expect("stdin");
    let mut stdout = process.take_stdout().expect("stdout");

    // Nothing has been written yet, so this read is cancelled by the timeout.
    let early = tokio::time::timeout(Duration::from_millis(50), stdout.next_line()).await;
    assert!(early.is_err(), "the read should still have been pending");

    stdin.write_line("after the cancel").await.expect("write");
    drop(stdin);
    assert_eq!(
        stdout.next_line().await.expect("read").as_deref(),
        Some("after the cancel")
    );
}

#[tokio::test]
async fn a_line_containing_a_newline_is_refused() {
    let mut process = spawn(echo()).await;
    let mut stdin = process.take_stdin().expect("stdin");
    let err = stdin.write_line("one\ntwo").await.expect_err("refused");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

#[tokio::test]
async fn stderr_is_captured_separately_and_the_exit_code_reported() {
    let mut process = spawn(shell("echo oops 1>&2 && exit 3")).await;
    let mut stderr = process.take_stderr().expect("stderr");
    let mut stdout = process.take_stdout().expect("stdout");

    let line = stderr.next_line().await.expect("read").expect("a line");
    assert_eq!(line.trim_end(), "oops");
    assert_eq!(stdout.next_line().await.expect("read"), None);

    let exit = process.wait().await.expect("wait");
    assert_eq!(exit.code, Some(3));
    assert!(!exit.success());
    // Waiting again gives the same answer.
    assert_eq!(process.wait().await.expect("wait again"), exit);
}

#[tokio::test]
async fn kill_ends_a_running_process_and_is_idempotent() {
    // Reads stdin forever, since nothing closes it.
    let mut process = spawn(echo()).await;
    let _stdin = process.take_stdin();

    process.kill().await.expect("kill");
    let exit = process.wait().await.expect("wait");
    assert!(!exit.success());
    process
        .kill()
        .await
        .expect("killing an exited process is fine");
}

#[tokio::test]
async fn env_and_cwd_reach_the_process() {
    let dir = std::env::temp_dir();
    let mut spec = if cfg!(windows) {
        shell("echo %ERSA_TEST_VAR% & cd")
    } else {
        shell("echo $ERSA_TEST_VAR; pwd")
    };
    spec.env.insert("ERSA_TEST_VAR".into(), "from-spec".into());
    spec.cwd = Some(dir.to_string_lossy().into_owned());

    let mut process = spawn(spec).await;
    let mut stdout = process.take_stdout().expect("stdout");
    let var = stdout.next_line().await.expect("read").expect("a line");
    let cwd = stdout.next_line().await.expect("read").expect("a line");

    assert_eq!(var.trim_end(), "from-spec");
    // Compare canonical forms: temp paths can differ in case or by symlink.
    let reported = std::fs::canonicalize(cwd.trim_end()).expect("cwd exists");
    assert_eq!(reported, std::fs::canonicalize(&dir).expect("temp dir"));
}

#[tokio::test]
async fn spawning_a_missing_program_fails() {
    let spec = ProcessSpec {
        program: "ersa-no-such-program".into(),
        ..ProcessSpec::default()
    };
    let result = LocalTransport.spawn(spec).await;
    assert_eq!(
        result.err().map(|e| e.kind()),
        Some(std::io::ErrorKind::NotFound)
    );
}

#[tokio::test]
async fn a_line_that_is_not_utf8_is_decoded_lossily_and_reading_continues() {
    // Writes `a`, the byte 0xFF, `b`, a newline, then `next` and a newline.
    let spec = if cfg!(windows) {
        ProcessSpec {
            program: "powershell".into(),
            args: vec![
                "-NoProfile".into(),
                "-Command".into(),
                "$b = [byte[]](0x61,0xFF,0x62,0x0A,0x6E,0x65,0x78,0x74,0x0A); \
                 [Console]::OpenStandardOutput().Write($b, 0, $b.Length)"
                    .into(),
            ],
            ..ProcessSpec::default()
        }
    } else {
        shell(r"printf 'a\377b\nnext\n'")
    };
    let mut process = spawn(spec).await;
    let mut stdout = process.take_stdout().expect("stdout");

    assert_eq!(
        stdout.next_line().await.expect("read").as_deref(),
        Some("a\u{FFFD}b")
    );
    assert_eq!(
        stdout.next_line().await.expect("read").as_deref(),
        Some("next")
    );
    assert_eq!(stdout.next_line().await.expect("read"), None);
}
