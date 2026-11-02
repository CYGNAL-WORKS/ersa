//! A stand-in for a backend CLI, for the Shell's quit tests: Core starts it
//! as Claude Code (`ERSA_CLAUDE`), so a test can quit while a session runs
//! without a real CLI or a model call.
//!
//! - With `--version`, it prints a version the way the CLI does, and exits.
//! - With `ERSA_STAND_IN_SLEEP` set, it sleeps: the grandchild below.
//! - Otherwise it starts that grandchild, writes `<own pid> <grandchild pid>`
//!   to the file named by `ERSA_STAND_IN_PIDS`, and reads its input until
//!   the input closes, which is how Core tells the CLI to stop. Its
//!   grandchild keeps sleeping: Core must end it with the session.
//!
//! An example so `cargo test` builds it beside the tests, which find it at
//! `target/<profile>/examples/`.

use std::io::Read;
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
    let mut input = Vec::new();
    let _ = std::io::stdin().lock().read_to_end(&mut input);
}
