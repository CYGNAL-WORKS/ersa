//! Core's local transport against real processes: a stopped backend leaves
//! nothing behind, grandchildren included (`ARCH-CORE`, Boundaries), and a
//! process cannot exhaust Core's memory with one endless line.

mod support;

use std::time::Duration;

use ersa_adapter::{Process, ProcessSpec, Transport};
use ersa_core::transport::LocalTreeTransport;
use support::procs::{eventually_gone, parent_of_a_grandchild, running, shell};

/// Spawns `spec` and reads the grandchild's pid from its first line.
async fn spawn_with_grandchild(spec: ProcessSpec) -> (Box<dyn Process>, u32) {
    spawn_with_grandchild_on(LocalTreeTransport::new(), spec).await
}

async fn spawn_with_grandchild_on(
    transport: LocalTreeTransport,
    spec: ProcessSpec,
) -> (Box<dyn Process>, u32) {
    let mut process = transport.spawn(spec).await.expect("spawn");
    let mut stdout = process.take_stdout().expect("stdout");
    let line = tokio::time::timeout(Duration::from_secs(30), stdout.next_line())
        .await
        .expect("the parent printed in time")
        .expect("read")
        .expect("a line");
    let pid: u32 = line
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("a pid, not {line:?}"));
    assert!(running(pid), "the grandchild started");
    (process, pid)
}

#[tokio::test(flavor = "multi_thread")]
async fn killing_a_process_kills_its_grandchildren() {
    let (mut process, grandchild) = spawn_with_grandchild(parent_of_a_grandchild(true)).await;
    process.kill().await.expect("kill");
    let exit = tokio::time::timeout(Duration::from_secs(10), process.wait())
        .await
        .expect("exited in time")
        .expect("wait");
    assert!(!exit.success());
    eventually_gone(grandchild).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backend_that_exits_by_itself_takes_its_grandchildren_with_it() {
    let (mut process, grandchild) = spawn_with_grandchild(parent_of_a_grandchild(false)).await;
    let exit = tokio::time::timeout(Duration::from_secs(30), process.wait())
        .await
        .expect("exited in time")
        .expect("wait");
    assert!(exit.success());
    // By the time the exit is reported, what it started is being killed.
    eventually_gone(grandchild).await;
    drop(process);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_may_leave_a_helper_running_but_is_killed_whole_on_a_timeout() {
    // A finished command keeps what it meant to leave running.
    let commands = LocalTreeTransport::for_commands();
    let (mut process, helper) =
        spawn_with_grandchild_on(commands, parent_of_a_grandchild(false)).await;
    process.wait().await.expect("wait");
    drop(process);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(running(helper), "the helper outlives the command");
    kill_by_pid(helper);
    eventually_gone(helper).await;

    // One that is killed takes its tree with it.
    let (mut process, grandchild) =
        spawn_with_grandchild_on(commands, parent_of_a_grandchild(true)).await;
    process.kill().await.expect("kill");
    eventually_gone(grandchild).await;
}

fn kill_by_pid(pid: u32) {
    let pid = pid.to_string();
    let _ = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid])
            .output()
    } else {
        std::process::Command::new("kill")
            .args(["-9", &pid])
            .output()
    };
}

#[tokio::test(flavor = "multi_thread")]
async fn an_endless_line_is_refused_and_its_writer_killed() {
    let transport = LocalTreeTransport::with_max_line_bytes(1000);
    let script = if cfg!(windows) {
        "[Console]::Out.Write('x' * 5000); [Console]::Out.Flush(); Start-Sleep 600"
    } else {
        "head -c 5000 /dev/zero | tr '\\0' x; sleep 600"
    };
    let mut process = transport.spawn(shell(script)).await.expect("spawn");
    let mut stdout = process.take_stdout().expect("stdout");
    let read = tokio::time::timeout(Duration::from_secs(30), stdout.next_line())
        .await
        .expect("refused in time");
    assert_eq!(
        read.expect_err("an over-long line is an error").kind(),
        std::io::ErrorKind::InvalidData
    );
    tokio::time::timeout(Duration::from_secs(10), process.wait())
        .await
        .expect("killed")
        .expect("wait");
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cmd_shim_on_the_path_is_found() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        dir.path().join("ersa-shim-test.cmd"),
        "@echo shim says %1\r\n",
    )
    .unwrap();
    let path = dir.path().display().to_string();
    let found = ersa_core::platform::resolve_program("ersa-shim-test", Some(&path));
    assert!(
        found.to_lowercase().ends_with("ersa-shim-test.cmd"),
        "{found}"
    );

    let spec = ProcessSpec {
        program: "ersa-shim-test".into(),
        args: vec!["hello".into()],
        env: [("PATH".to_string(), path)].into(),
        ..ProcessSpec::default()
    };
    let mut process = LocalTreeTransport::new().spawn(spec).await.expect("spawn");
    let mut stdout = process.take_stdout().expect("stdout");
    let line = stdout.next_line().await.expect("read");
    assert_eq!(line.as_deref(), Some("shim says hello"));
}
