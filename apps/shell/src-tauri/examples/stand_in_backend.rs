//! A stand-in for a backend CLI, for the Shell's tests: Core starts it
//! as Claude Code (`ERSA_CLAUDE`), so a test can start, quit or fail a
//! session without a real CLI or a model call.
//!
//! - With `--version`, it prints a version the way the CLI does, and exits.
//! - With `ERSA_STAND_IN_SLEEP` set, it sleeps: the grandchild below.
//! - With `ERSA_STAND_IN_FAIL` set, it fails to start: it writes the
//!   variable's value to stderr, as a CLI says why it cannot run, and exits 1.
//! - With `ERSA_STAND_IN_ANSWER` set, it answers the host's `initialize`
//!   request with one model, the CLI's own `default` entry, so detection
//!   lists it and the session starts and takes messages.
//! - Otherwise it starts that grandchild, writes `<own pid> <grandchild pid>`
//!   to the file named by `ERSA_STAND_IN_PIDS`, and reads its input until
//!   the input closes, which is how Core tells the CLI to stop. Its
//!   grandchild keeps sleeping: Core must end it with the session.
//!
//! An example so `cargo test` builds it beside the tests, which find it at
//! `target/<profile>/examples/`.

use std::io::{BufRead, Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

fn main() {
    if std::env::args().any(|arg| arg == "--version") {
        println!("2.1.302 (Claude Code)");
        return;
    }
    if std::env::var_os("ERSA_STAND_IN_SLEEP").is_some() {
        std::thread::sleep(Duration::from_secs(600));
        return;
    }
    if let Some(message) = std::env::var_os("ERSA_STAND_IN_FAIL") {
        eprintln!("{}", message.to_string_lossy());
        std::process::exit(1);
    }
    let me = std::env::current_exe().expect("the stand-in's own path");
    // Never waited on: it outlives this process until Core ends it.
    #[allow(clippy::zombie_processes)]
    let grandchild = Command::new(me)
        .env("ERSA_STAND_IN_SLEEP", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the grandchild");
    if let Some(path) = std::env::var_os("ERSA_STAND_IN_PIDS") {
        // Written whole and then renamed, so a reader never sees half of it.
        let path = std::path::PathBuf::from(path);
        let partial = path.with_extension("partial");
        let pids = format!("{} {}", std::process::id(), grandchild.id());
        std::fs::write(&partial, pids).expect("write the pids");
        std::fs::rename(&partial, &path).expect("publish the pids");
    }
    if std::env::var_os("ERSA_STAND_IN_ANSWER").is_some() {
        answer_initialize();
    }
    let mut input = Vec::new();
    let _ = std::io::stdin().lock().read_to_end(&mut input);
}

/// Reads the host's lines until `initialize`, and answers it with success
/// and the CLI's `default` model entry (PROTO-CLAUDE-CODE-STREAM, the
/// handshake); what follows is read and ignored by the caller.
fn answer_initialize() {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { return };
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if message["type"] == "control_request" && message["request"]["subtype"] == "initialize" {
            let answer = serde_json::json!({
                "type": "control_response",
                "response": {
                    "subtype": "success",
                    "request_id": message["request_id"],
                    "response": {
                        "models": [{
                            "value": "default",
                            "displayName": "Default (recommended)",
                            "resolvedModel": "stand-in-model",
                            "supportedEffortLevels": ["low", "high"],
                        }],
                    },
                },
            });
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{answer}");
            let _ = out.flush();
            return;
        }
    }
}
