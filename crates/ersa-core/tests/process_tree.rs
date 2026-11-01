//! Core's local transport against real processes: a stopped backend leaves
//! nothing behind, grandchildren included (`ARCH-CORE`, Boundaries), and a
//! process cannot exhaust Core's memory with one endless line.

use std::time::Duration;

use ersa_adapter::{Process, ProcessSpec, Transport};
use ersa_core::transport::LocalTreeTransport;

/// A shell command, as this platform runs one.
fn shell(script: &str) -> ProcessSpec {
    if cfg!(windows) {
        ProcessSpec {
            program: "powershell".into(),
            args: vec!["-NoProfile".into(), "-Command".into(), script.into()],
            ..ProcessSpec::default()
        }
    } else {
        ProcessSpec {
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            ..ProcessSpec::default()
        }
    }
}

/// Starts a long-lived grandchild, prints its process id, and either keeps
/// running (`then_wait`) or exits at once, leaving the grandchild behind.
fn parent_of_a_grandchild(then_wait: bool) -> ProcessSpec {
    if cfg!(windows) {
        let wait = if then_wait { "; Start-Sleep 600" } else { "" };
        shell(&format!(
            "$p = Start-Process -PassThru -WindowStyle Hidden ping -ArgumentList '-n','600','127.0.0.1'; \
             [Console]::Out.WriteLine($p.Id); [Console]::Out.Flush(){wait}"
        ))
    } else {
        let wait = if then_wait { "; wait" } else { "" };
        shell(&format!("sleep 600 & echo $!{wait}"))
    }
}

/// Whether process `pid` is still running.
fn running(pid: u32) -> bool {
    if cfg!(windows) {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .expect("tasklist runs");
        // A found process is listed as `"name","<pid>",...`; the message for
        // none is localised, so look for the quoted id instead.
        String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
    } else {
        // Gone, or a zombie waiting for init to reap it, is not running.
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => !stat
                .rsplit(')')
                .next()
                .is_some_and(|rest| rest.trim_start().starts_with('Z')),
            Err(_) => false,
        }
    }
}

async fn eventually_gone(pid: u32) {
    for _ in 0..100 {
        if !running(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("process {pid} is still running");
}

/// Spawns `spec` and reads the grandchild's pid from its first line.
async fn spawn_with_grandchild(spec: ProcessSpec) -> (Box<dyn Process>, u32) {
    let mut process = LocalTreeTransport::new().spawn(spec).await.expect("spawn");
    let mut stdout = process.take_stdout().expect("stdout");
    let line = tokio::time::timeout(Duration::from_secs(30), stdout.next_line())
        .await
        .expect("the parent printed in time")
        .expect("read")
        .expect("a line");
    let pid: u32 = line.trim().parse().unwrap_or_else(|_| panic!("a pid, not {line:?}"));
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
async fn a_process_that_exits_by_itself_takes_its_grandchildren_when_dropped() {
    let (mut process, grandchild) = spawn_with_grandchild(parent_of_a_grandchild(false)).await;
    let exit = tokio::time::timeout(Duration::from_secs(30), process.wait())
        .await
        .expect("exited in time")
        .expect("wait");
    assert!(exit.success());
    // The parent is gone; what it started is not, until Core lets go.
    assert!(running(grandchild));
    drop(process);
    eventually_gone(grandchild).await;
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
    std::fs::write(dir.path().join("ersa-shim-test.cmd"), "@echo shim says %1\r\n").unwrap();
    let path = dir.path().display().to_string();
    let found = ersa_core::platform::resolve_program("ersa-shim-test", Some(&path));
    assert!(found.to_lowercase().ends_with("ersa-shim-test.cmd"), "{found}");

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
