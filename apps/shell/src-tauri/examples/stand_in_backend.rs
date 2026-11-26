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
//!   lists it and the session starts and takes messages; it answers
//!   `get_settings` with `low` as the effort a session given none runs
//!   with, so detection marks it the default effort; it then accepts
//!   each change of model, permission mode or effort as the CLI does.
//!   With `CLAUDE_CONFIG_DIR` set, its first message writes a session record
//!   under the `--session-id` it was started with, as the CLI does, so
//!   Core's look finds the session there (FLOW-RESUME-SESSION-R18); the
//!   record names no folder, so it is never listed as a session started
//!   outside Ersa. Sent the message `limits`, it reports the plan's usage windows as the
//!   CLI's `rate_limit_event` does, with made-up shares: a 5-hour limit at
//!   84 percent resetting in 1 hr 15 min, and a weekly limit at 62 percent
//!   resetting in three days.
//!   With `ERSA_STAND_IN_SIGNED_OUT` set too, its `initialize` answer
//!   carries the account a signed-out CLI reports, so Core lists it as
//!   signed out (PROTO-CLAUDE-CODE-STREAM-R83).
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

/// Whether a `user` line from the host says exactly `text`, given as a
/// string or as one text block.
fn said(message: &serde_json::Value, text: &str) -> bool {
    let content = &message["message"]["content"];
    content.as_str() == Some(text)
        || content
            .as_array()
            .is_some_and(|blocks| blocks.iter().any(|b| b["text"].as_str() == Some(text)))
}

/// Writes the session's record, one user entry, under
/// `$CLAUDE_CONFIG_DIR/projects/stand-in/<--session-id>.jsonl`, once: the
/// CLI files a record with its first message. Without either, nothing.
fn write_record() {
    let Some(home) = std::env::var_os("CLAUDE_CONFIG_DIR") else {
        return;
    };
    let args: Vec<String> = std::env::args().collect();
    let Some(id) = args
        .iter()
        .position(|a| a == "--session-id")
        .and_then(|at| args.get(at + 1))
    else {
        return;
    };
    let dir = std::path::Path::new(&home)
        .join("projects")
        .join("stand-in");
    let file = dir.join(format!("{id}.jsonl"));
    if file.exists() {
        return;
    }
    // A bookkeeping entry the CLI writes first, which holds no prompt: the
    // record then gives the session no title, as the stand-in's stream does.
    // Stamped with the time it is written, as the CLI stamps it: Core takes
    // the session's creation time from it, so two sessions started one after
    // the other are as old as they are, and the newer one is the newest.
    let line = serde_json::json!({
        "type": "queue-operation",
        "operation": "enqueue",
        "timestamp": ersa_core::persistence::now_rfc3339(),
        "sessionId": id,
    });
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(
        file,
        format!(
            "{line}
"
        ),
    );
}

/// Writes a `rate_limit_event` line with two usage windows, made up, whose
/// reset times are counted from now so the window shows each way of saying
/// when a limit resets (docs/design/COMPOSER.md, The context ring).
fn report_limits() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs();
    let line = serde_json::json!({
        "type": "rate_limit_event",
        "session_id": "stand-in",
        "uuid": "stand-in-limits",
        "rate_limit_info": {
            "status": "allowed_warning",
            "unifiedWindows": {
                // Just under 1 hr 15 min, which the list rounds up to it for the next half minute.
                "five_hour": { "utilization": 0.84, "resetsAt": now + 75 * 60 - 30 },
                "seven_day": { "utilization": 0.62, "resetsAt": now + 3 * 24 * 3600 },
            },
        },
    });
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// Reads the host's lines, answering `initialize` with success and the
/// CLI's `default` model entry (PROTO-CLAUDE-CODE-STREAM, the handshake),
/// `get_settings` with `low` applied as the effort, and each change of
/// model, permission mode or effort with success, the mode echoed as the
/// CLI answers it; anything else is read and ignored.
fn answer_initialize() {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { return };
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if message["type"] == "user" {
            write_record();
            if said(&message, "limits") {
                report_limits();
            }
            continue;
        }
        if message["type"] != "control_request" {
            continue;
        }
        let response = match message["request"]["subtype"].as_str() {
            Some("set_permission_mode") => {
                serde_json::json!({ "mode": message["request"]["mode"] })
            }
            Some("set_model" | "apply_flag_settings") => serde_json::json!({}),
            Some("get_settings") => serde_json::json!({ "applied": { "effort": "low" } }),
            _ => serde_json::Value::Null,
        };
        if !response.is_null() {
            let answer = serde_json::json!({
                "type": "control_response",
                "response": { "subtype": "success", "request_id": message["request_id"], "response": response },
            });
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{answer}");
            let _ = out.flush();
            continue;
        }
        if message["request"]["subtype"] == "initialize" {
            let mut answer = serde_json::json!({
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
            // The shape a signed-out CLI was recorded giving: no token, the first-party provider, and no key.
            if std::env::var_os("ERSA_STAND_IN_SIGNED_OUT").is_some() {
                answer["response"]["response"]["account"] =
                    serde_json::json!({ "tokenSource": "none", "apiProvider": "firstParty" });
            }
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{answer}");
            let _ = out.flush();
        }
    }
}
