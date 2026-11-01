//! Real processes for the process-tree tests: a parent that starts a
//! long-lived grandchild, and a way to see whether a process still runs.

use std::time::Duration;

use ersa_adapter::ProcessSpec;

/// A shell command, as this platform runs one.
pub fn shell(script: &str) -> ProcessSpec {
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
pub fn parent_of_a_grandchild(then_wait: bool) -> ProcessSpec {
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
pub fn running(pid: u32) -> bool {
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

/// Waits up to ten seconds for process `pid` to stop running.
pub async fn eventually_gone(pid: u32) {
    for _ in 0..100 {
        if !running(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("process {pid} is still running");
}
