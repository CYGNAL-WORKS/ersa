//! Finding Core: attach to the Core process already serving this user, or
//! launch `ersa-core serve` and attach to that (ADR 0011).
//!
//! The launched Core is detached from the Shell: it gets no console, its own
//! process group, and its output goes to a log file, so closing the window
//! (or the terminal the Shell was started from) does not stop it.

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use ersa_core::platform::BoxStream;

/// How long a launched Core may take to start serving before we give up.
pub const START_TIMEOUT: Duration = Duration::from_secs(15);

/// How often we try to connect while a launched Core starts.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Where Core is served and, if it is not running, how to start it.
#[derive(Debug, Clone)]
pub struct CoreLocation {
    /// The `ersa-core` executable.
    pub program: PathBuf,
    /// The named pipe or socket path Core serves.
    pub endpoint: String,
    /// Arguments after `serve`, such as `--data-dir`.
    pub serve_args: Vec<OsString>,
    /// Extra environment for the launched Core.
    pub env: Vec<(OsString, OsString)>,
    /// Where the launched Core's stdout and stderr go. `None` discards them.
    pub log_file: Option<PathBuf>,
    pub start_timeout: Duration,
    /// Whether to start Core when nothing serves the endpoint. `false` for a
    /// Core someone else runs, such as a replay server: then nothing serving
    /// is an error, never a reason to start the user's Core there.
    pub launch: bool,
}

/// Whether we found Core running or started it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Attached,
    Launched,
}

impl How {
    pub fn as_str(self) -> &'static str {
        match self {
            How::Attached => "attached",
            How::Launched => "launched",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    /// Something serves the endpoint but we may not talk to it: another
    /// user's process (the client refuses it), or a pipe that stayed busy.
    #[error("cannot attach to Core at {endpoint}: {source}")]
    Attach { endpoint: String, source: io::Error },
    #[error("cannot start Core ({program}): {source}")]
    Spawn { program: String, source: io::Error },
    /// The launched Core exited before serving. `log` is the end of its output.
    #[error("Core exited before it started serving ({status}){log}")]
    Exited { status: ExitStatus, log: String },
    #[error("Core did not start serving at {endpoint} within {timeout:?}")]
    Timeout { endpoint: String, timeout: Duration },
    /// The location may not launch, and nothing serves it.
    #[error(
        "nothing serves {endpoint}; start it first (a replay: ersa-core replay --fixture <path>)"
    )]
    NotServing { endpoint: String },
}

impl CoreLocation {
    /// The location the application uses: the user's default endpoint, Core's
    /// default data directory, and the executable named by `ERSA_CORE_BIN` or
    /// installed next to the Shell's own.
    pub fn default_for_user() -> io::Result<CoreLocation> {
        let log_file = ersa_core::platform::default_data_dir().map(|dir| dir.join("core.log"));
        Ok(CoreLocation {
            program: default_program()?,
            endpoint: ersa_core::platform::default_endpoint()?,
            serve_args: Vec::new(),
            env: Vec::new(),
            log_file,
            start_timeout: START_TIMEOUT,
            launch: true,
        })
    }

    /// A Core someone else runs on the user's endpoint labelled `label`
    /// (`ersa-core serve` or `replay --endpoint <label>`): attach only,
    /// never launch.
    pub fn attach_only(label: &str) -> io::Result<CoreLocation> {
        Ok(CoreLocation {
            program: PathBuf::new(),
            endpoint: ersa_core::platform::labelled_endpoint(label)?,
            serve_args: Vec::new(),
            env: Vec::new(),
            log_file: None,
            start_timeout: START_TIMEOUT,
            launch: false,
        })
    }
}

/// `ersa-core` beside the running executable. A debug build also honours
/// `ERSA_CORE_BIN`, and otherwise falls back to the root workspace's debug
/// build, which is where `cargo build -p ersa-cli` puts it. A release build
/// ignores the environment: it runs only the Core installed with it.
fn default_program() -> io::Result<PathBuf> {
    if cfg!(debug_assertions)
        && let Some(path) = std::env::var_os("ERSA_CORE_BIN")
    {
        return Ok(PathBuf::from(path));
    }
    let name = format!("ersa-core{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe()?.with_file_name(&name);
    if beside.exists() || !cfg!(debug_assertions) {
        return Ok(beside);
    }
    // CARGO_MANIFEST_DIR is apps/shell/src-tauri at compile time.
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../target/debug")
        .join(name))
}

/// Connects to the Core serving `location.endpoint`, starting one first if
/// none is. The connection is already verified as served by the current
/// user (the pipe client checks that), but no request has been sent.
pub async fn attach_or_launch(location: &CoreLocation) -> Result<(BoxStream, How), LaunchError> {
    attach_or_launch_tracked(location)
        .await
        .map(|(stream, how, _)| (stream, how))
}

/// [`attach_or_launch`], with the process id of the Core it launched, if it
/// launched one: Quit needs it when a wedged Core can no longer be found
/// from its endpoint.
pub async fn attach_or_launch_tracked(
    location: &CoreLocation,
) -> Result<(BoxStream, How, Option<u32>), LaunchError> {
    match ersa_core::connect(&location.endpoint).await {
        Ok(stream) => return Ok((stream, How::Attached, None)),
        Err(e) if nobody_serving(&e) && !location.launch => {
            return Err(LaunchError::NotServing {
                endpoint: location.endpoint.clone(),
            });
        }
        Err(e) if nobody_serving(&e) => {}
        Err(source) => {
            return Err(LaunchError::Attach {
                endpoint: location.endpoint.clone(),
                source,
            });
        }
    }

    // Where this run's output starts in the shared, append-only log, so a
    // failure shows its own output and not an earlier run's.
    if let Some(path) = &location.log_file {
        rotate_log(path);
    }
    let log_start = location
        .log_file
        .as_deref()
        .and_then(|path| std::fs::metadata(path).ok())
        .map_or(0, |meta| meta.len());
    let mut child = spawn(location)?;
    let deadline = tokio::time::Instant::now() + location.start_timeout;
    loop {
        // Try to connect before looking at the child: when two Shells launch
        // at once, one Core loses the race and exits, but the other serves.
        match ersa_core::connect(&location.endpoint).await {
            Ok(stream) => {
                let pid = child.id();
                reap(child);
                return Ok((stream, How::Launched, Some(pid)));
            }
            Err(e) if nobody_serving(&e) => {}
            Err(source) => {
                reap(child);
                return Err(LaunchError::Attach {
                    endpoint: location.endpoint.clone(),
                    source,
                });
            }
        }
        if let Ok(Some(status)) = child.try_wait() {
            // One last try, for the lost race above.
            if let Ok(stream) = ersa_core::connect(&location.endpoint).await {
                return Ok((stream, How::Attached, None));
            }
            let log = location
                .log_file
                .as_deref()
                .map(|path| log_tail(path, log_start))
                .unwrap_or_default();
            return Err(LaunchError::Exited { status, log });
        }
        if tokio::time::Instant::now() >= deadline {
            // A Core that hangs before serving would otherwise linger, and
            // each retry would add another.
            let _ = child.kill();
            let _ = child.wait();
            return Err(LaunchError::Timeout {
                endpoint: location.endpoint.clone(),
                timeout: location.start_timeout,
            });
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// The errors that mean no Core is listening: no pipe or socket by that name,
/// or a socket file left by a Core that is gone.
fn nobody_serving(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

/// Waits for the launched Core on a thread of its own, so that when it exits
/// (long after the Shell has attached) it leaves no zombie behind on Unix.
/// Waiting neither stops Core nor ties its lifetime to the Shell's.
fn reap(mut child: std::process::Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

fn spawn(location: &CoreLocation) -> Result<std::process::Child, LaunchError> {
    let spawn_error = |source| LaunchError::Spawn {
        program: location.program.display().to_string(),
        source,
    };
    let command = |breakaway: bool| -> io::Result<Command> {
        let (stdout, stderr) = match &location.log_file {
            Some(path) => {
                let file = open_log(path)?;
                let copy = file.try_clone()?;
                (Stdio::from(file), Stdio::from(copy))
            }
            None => (Stdio::null(), Stdio::null()),
        };
        let mut command = Command::new(&location.program);
        command
            .arg("serve")
            .args(&location.serve_args)
            .envs(location.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr);
        detach(&mut command, breakaway);
        Ok(command)
    };
    // Core must outlive the Shell even when the Shell runs inside a job that
    // kills its processes on close (some terminals and launchers do that), so
    // ask to leave the job; a job that forbids leaving refuses the spawn, and
    // then Core starts inside it.
    match command(true).map_err(spawn_error)?.spawn() {
        Ok(child) => Ok(child),
        Err(e) if cfg!(windows) && e.kind() == io::ErrorKind::PermissionDenied => command(false)
            .map_err(spawn_error)?
            .spawn()
            .map_err(spawn_error),
        Err(e) => Err(spawn_error(e)),
    }
}

/// The log is kept to about [`MAX_LOG_BYTES`]: past that, the old log moves
/// to `core.log.1` (replacing the one before) and a new log starts. A Core
/// still writing to the old file (on Windows, one holding it open) keeps it.
const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;

fn rotate_log(path: &Path) {
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
}

fn open_log(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// The last lines the log gained after byte `start`, prefixed for appending
/// to an error message. Reads at most the last 16 KiB of that.
fn log_tail(path: &Path, start: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    const LINES: usize = 10;
    const MAX_BYTES: u64 = 16 * 1024;
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let end = file.metadata().map_or(start, |meta| meta.len());
    let from = start.max(end.saturating_sub(MAX_BYTES));
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(from)).is_err() || file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    // Core writes UTF-8; anything else is shown with replacement characters
    // rather than dropped.
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(LINES)..].join("\n");
    if tail.is_empty() {
        String::new()
    } else {
        format!(":\n{tail}")
    }
}

/// Windows: no console window, and a process group of its own so a Ctrl+C in
/// the terminal that started the Shell does not reach Core; with `breakaway`,
/// outside the Shell's job as well.
#[cfg(windows)]
fn detach(command: &mut Command, breakaway: bool) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let mut flags = CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
    if breakaway {
        flags |= CREATE_BREAKAWAY_FROM_JOB;
    }
    command.creation_flags(flags);
}

/// Unix: a process group of its own, for the same reason. Jobs are a Windows
/// idea, so `breakaway` means nothing here.
#[cfg(unix)]
fn detach(command: &mut Command, _breakaway: bool) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}
