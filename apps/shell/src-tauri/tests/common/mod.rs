//! Helpers shared by the Shell's integration tests. A `tests/common/`
//! module is Cargo's convention for code several test files include
//! (`mod common;`) without becoming a test binary of its own.

#![allow(dead_code)] // each test file uses only some of these

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use ersa_shell::bridge::{CoreLink, LinkEvent, LinkOptions};
use ersa_shell::launch::{self, CoreLocation, How};
use ersa_shell::quit::{self, QUIT_LIMIT};
use serde_json::Value;
use tokio::sync::mpsc;

/// Bounds on what a test waits for, well above what each takes.
pub const OPEN: Duration = Duration::from_secs(90);
pub const CLEANUP: Duration = Duration::from_secs(30);

/// The `ersa-core` executable: `ERSA_CORE_BIN`, or the root workspace's
/// debug build (`cargo build -p ersa-cli` at the repository root).
pub fn core_program() -> PathBuf {
    let program = std::env::var_os("ERSA_CORE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            repo_root()
                .join("target/debug")
                .join(format!("ersa-core{}", std::env::consts::EXE_SUFFIX))
        });
    assert!(
        program.exists(),
        "no ersa-core at {}: run `cargo build -p ersa-cli` at the repository root, or set ERSA_CORE_BIN",
        program.display()
    );
    program
}

/// The repository root. CARGO_MANIFEST_DIR is apps/shell/src-tauri.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// An endpoint label no other test, and no Core of the user's, serves: the
/// process id and a counter.
pub fn unique_label(prefix: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// A Core location private to a test: the user's endpoint labelled with the
/// process id and a counter (`serve --endpoint <label>`), and a data
/// directory and log in `dir`.
pub fn private_location(dir: &Path) -> CoreLocation {
    end_with_this_run();
    let label = unique_label("shell-test");
    let endpoint = ersa_core::platform::labelled_endpoint(&label).expect("an endpoint");
    CoreLocation {
        program: core_program(),
        endpoint,
        serve_args: vec![
            "--endpoint".into(),
            label.into(),
            "--data-dir".into(),
            dir.join("data").into_os_string(),
        ],
        env: Vec::new(),
        log_file: Some(dir.join("core.log")),
        start_timeout: Duration::from_secs(60),
        launch: true,
    }
}

/// Puts this test process in a job object that ends every process in it
/// when the process ends, however it ends: a panic, a timeout, Ctrl+C. A
/// Core the tests launch asks to leave the Shell's job; this job does not
/// allow that, so Core starts inside it (the launcher's fallback for such a
/// job) and cannot outlive the run. Unix has no such job; there, each test
/// stops its own Core when it ends.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn end_with_this_run() {
    use std::sync::Once;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: plain calls with valid arguments; the job handle is left
        // open on purpose, since closing it is what ends the job's processes.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            assert!(!job.is_null(), "create a job object");
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            assert!(set != 0, "set the job's limits");
            let joined = AssignProcessToJobObject(job, GetCurrentProcess());
            assert!(joined != 0, "join the job");
        }
    });
}

#[cfg(unix)]
pub fn end_with_this_run() {}

/// The stand-in backend CLI (examples/stand_in_backend.rs), which `cargo
/// test` builds beside this test: tests live in `target/<profile>/deps/`.
pub fn stand_in_backend() -> PathBuf {
    let exe = std::env::current_exe().expect("this test binary");
    let path = exe
        .parent()
        .and_then(Path::parent)
        .expect("target/<profile>")
        .join("examples")
        .join(format!("stand_in_backend{}", std::env::consts::EXE_SUFFIX));
    assert!(path.exists(), "no stand-in backend at {}", path.display());
    path
}

pub async fn open(location: &CoreLocation) -> (CoreLink, How, mpsc::UnboundedReceiver<LinkEvent>) {
    let opening = async {
        let (stream, how) = launch::attach_or_launch(location)
            .await
            .unwrap_or_else(|e| panic!("attach or launch: {e}"));
        let (events, received) = mpsc::unbounded_channel();
        let link = CoreLink::open(stream, LinkOptions::default(), move |event| {
            let _ = events.send(event);
        })
        .await
        .unwrap_or_else(|e| panic!("handshake: {e}"));
        (link, how, received)
    };
    tokio::time::timeout(OPEN, opening)
        .await
        .unwrap_or_else(|_| panic!("no Core answered at {}", location.endpoint))
}

pub async fn request(link: &CoreLink, method: &str, params: Value) -> Value {
    link.request(method, Some(params))
        .await
        .unwrap_or_else(|e| panic!("{method}: {e}"))
}

/// Waits for `checks`, then stops whatever Core still serves the location,
/// however the checks ended: a launched Core is detached, so nothing else
/// would stop it.
pub async fn finish(checks: tokio::task::JoinHandle<()>, location: &CoreLocation) {
    let outcome = checks.await;
    if tokio::time::timeout(
        CLEANUP,
        quit::shut_down(&location.endpoint, QUIT_LIMIT, None),
    )
    .await
    .is_err()
    {
        eprintln!("could not stop the Core at {}", location.endpoint);
    }
    if let Err(failed) = outcome {
        std::panic::resume_unwind(failed.into_panic());
    }
}

/// A git repository with one commit, for a project whose workspace is a
/// worktree.
pub fn git_repo(dir: &Path) {
    std::fs::create_dir_all(dir).expect("create the repository");
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Ersa test")
            .env("GIT_AUTHOR_EMAIL", "test@ersa.invalid")
            .env("GIT_COMMITTER_NAME", "Ersa test")
            .env("GIT_COMMITTER_EMAIL", "test@ersa.invalid")
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(dir.join("README.md"), "test repository\n").expect("write a file");
    git(&["add", "README.md"]);
    git(&["commit", "--quiet", "-m", "initial"]);
}
