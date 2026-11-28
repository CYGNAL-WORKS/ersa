//! Real processes for the process-tree tests: a parent that starts a
//! long-lived grandchild, and a way to see whether a process still runs.

use std::io;
use std::time::Duration;

use ersa_adapter::{LineReader, ProcessSpec};

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
/// Read the id with [`grandchild_pid`].
///
/// On Windows the parent is this test binary run again, which starts in
/// milliseconds, where PowerShell takes half a second on an idle machine
/// and seconds on a busy one: a sign-in test's start limit measured
/// PowerShell's start-up instead of Core (#664). Elsewhere `sh` starts as
/// fast.
pub fn parent_of_a_grandchild(then_wait: bool) -> ProcessSpec {
    if cfg!(windows) {
        let part = if then_wait {
            PARENT_THAT_WAITS
        } else {
            PARENT_THAT_EXITS
        };
        ProcessSpec {
            program: std::env::current_exe().unwrap().display().to_string(),
            // Only the test that plays the part, its output not captured.
            args: [PART_TEST, "--exact", "--ignored", "--nocapture", "-q"]
                .map(String::from)
                .to_vec(),
            env: [(PART.to_string(), part.to_string())].into(),
            ..ProcessSpec::default()
        }
    } else {
        let wait = if then_wait { "; wait" } else { "" };
        shell(&format!("sleep 600 & echo $!{wait}"))
    }
}

/// The grandchild's process id, from the output of a process
/// [`parent_of_a_grandchild`] started: the first line that is a number,
/// since on Windows the test binary prints lines of its own before it.
/// `None` if the output ended without one.
pub async fn grandchild_pid(stdout: &mut dyn LineReader) -> io::Result<Option<u32>> {
    while let Some(line) = stdout.next_line().await? {
        if let Ok(pid) = line.trim().parse() {
            return Ok(Some(pid));
        }
    }
    Ok(None)
}

/// The variable that tells this test binary, run again by
/// [`parent_of_a_grandchild`], which part it plays.
const PART: &str = "ERSA_TEST_PROCESS_PART";
const PARENT_THAT_WAITS: &str = "parent-that-waits";
const PARENT_THAT_EXITS: &str = "parent-that-exits";

/// [`plays_a_part_in_a_process_tree`]'s full name in every test binary,
/// each of which has this module as `support::procs`.
const PART_TEST: &str = "support::procs::plays_a_part_in_a_process_tree";

/// The parent [`parent_of_a_grandchild`] starts on Windows: it starts
/// `ping`, a long-lived grandchild, prints its id, then waits or exits.
/// Run any other way, it does nothing.
#[test]
#[ignore = "a process for the process-tree tests, which start it themselves"]
fn plays_a_part_in_a_process_tree() {
    use std::io::Write;
    let part = std::env::var(PART).unwrap_or_default();
    if part != PARENT_THAT_WAITS && part != PARENT_THAT_EXITS {
        return;
    }
    let grandchild = std::process::Command::new("ping")
        .args(["-n", "600", "127.0.0.1"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("ping starts");
    let mut out = std::io::stdout();
    writeln!(out, "{}", grandchild.id()).unwrap();
    out.flush().unwrap();
    if part == PARENT_THAT_WAITS {
        std::thread::sleep(Duration::from_secs(600));
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
