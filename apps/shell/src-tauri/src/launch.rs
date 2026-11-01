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
        })
    }
}

/// `ERSA_CORE_BIN`, else `ersa-core` beside the running executable, else (in a
/// debug build) the root workspace's debug build of it, which is where
/// `cargo build -p ersa-cli` puts it during development.
fn default_program() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("ERSA_CORE_BIN") {
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
    match ersa_core::connect(&location.endpoint).await {
        Ok(stream) => return Ok((stream, How::Attached)),
        Err(e) if nobody_serving(&e) => {}
        Err(source) => {
            return Err(LaunchError::Attach {
                endpoint: location.endpoint.clone(),
                source,
            });
        }
    }

    let mut child = spawn(location)?;
    let deadline = tokio::time::Instant::now() + location.start_timeout;
    loop {
        // Try to connect before looking at the child: when two Shells launch
        // at once, one Core loses the race and exits, but the other serves.
        match ersa_core::connect(&location.endpoint).await {
            Ok(stream) => return Ok((stream, How::Launched)),
            Err(e) if nobody_serving(&e) => {}
            Err(source) => {
                return Err(LaunchError::Attach {
                    endpoint: location.endpoint.clone(),
                    source,
                });
            }
        }
        if let Ok(Some(status)) = child.try_wait() {
            // One last try, for the lost race above.
            if let Ok(stream) = ersa_core::connect(&location.endpoint).await {
                return Ok((stream, How::Attached));
            }
            let log = location
                .log_file
                .as_deref()
                .map(log_tail)
                .unwrap_or_default();
            return Err(LaunchError::Exited { status, log });
        }
        if tokio::time::Instant::now() >= deadline {
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

fn spawn(location: &CoreLocation) -> Result<std::process::Child, LaunchError> {
    let spawn_error = |source| LaunchError::Spawn {
        program: location.program.display().to_string(),
        source,
    };
    let (stdout, stderr) = match &location.log_file {
        Some(path) => {
            let file = open_log(path).map_err(spawn_error)?;
            let copy = file.try_clone().map_err(spawn_error)?;
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
    detach(&mut command);
    // The child handle is dropped once Core serves; dropping it neither
    // waits for nor kills the process.
    command.spawn().map_err(spawn_error)
}

fn open_log(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// The last lines of the log, prefixed for appending to an error message.
fn log_tail(path: &Path) -> String {
    const LINES: usize = 10;
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(LINES)..].join("\n");
    if tail.is_empty() {
        String::new()
    } else {
        format!(":\n{tail}")
    }
}

/// Windows: no console window, and a process group of its own so a Ctrl+C in
/// the terminal that started the Shell does not reach Core.
#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

/// Unix: a process group of its own, for the same reason.
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}
