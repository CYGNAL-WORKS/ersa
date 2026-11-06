//! Windows: named pipes that only the current user can open, and clients that
//! talk only to a pipe the current user serves (`PROTO-CORE-API`, Transport
//! and framing).
//!
//! Two protections, one per direction:
//!
//! - The server gives the pipe a security descriptor granting the current
//!   user alone. Without one, Windows' default descriptor lets other accounts
//!   on the machine open the pipe for reading.
//! - The client checks who serves the pipe before sending anything. Another
//!   account could create a pipe with Core's name before Core starts (pipe
//!   names are a machine-wide namespace); the client would then hand that
//!   account its requests. So the client asks Windows for the serving
//!   process and compares that process's user with its own.
//!
//! The pipe's name carries the user's SID rather than the user name, because
//! the user name comes from the environment, which anything can set.

// Calling Win32 functions is `unsafe`; the workspace denies unsafe code
// everywhere except where a platform module opts in, as this one does.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path, PathBuf, Prefix};
use std::ptr;
use std::time::Duration;

use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, HANDLE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::BoxStream;

/// How long `connect` keeps retrying while every pipe instance is busy.
const CONNECT_DEADLINE: Duration = Duration::from_secs(5);

/// `\\.\pipe\ersa-core-<user SID>`.
pub fn default_endpoint() -> io::Result<String> {
    Ok(format!(r"\\.\pipe\ersa-core-{}", current_user_sid()?))
}

/// `\\.\pipe\ersa-core-<user SID>-<label>`: an endpoint of this user's that
/// is not the default, for tests and a second Core. See
/// [`super::check_label`] for what `label` may hold.
pub fn labelled_endpoint(label: &str) -> io::Result<String> {
    super::check_label(label)?;
    Ok(format!("{}-{label}", default_endpoint()?))
}

/// `%LOCALAPPDATA%\Ersa`. Local rather than roaming app data, because the
/// database holds paths and raw logs specific to this machine.
pub fn default_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Ersa"))
}

/// The computer name.
pub fn host_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".to_string())
}

/// The canonical form of an existing path: absolute, with `.`, `..`, and
/// links resolved, and the letter case the file system stores.
///
/// `std::fs::canonicalize` returns Windows' verbatim form (`\\?\C:\repo`,
/// `\\?\UNC\server\share\repo`), which many tools, git among them, do not
/// accept. This turns it back into the ordinary form (`C:\repo`,
/// `\\server\share\repo`).
/// Whether `path` names a named pipe or a device: one in the `\\.\` or
/// non-drive `\\?\` namespaces, or a reserved device name (`NUL`, `COM1`,
/// with any extension) in any folder. Windows reports such a path as a file,
/// and asking about it connects to whatever serves it, so it is told by its
/// name alone.
pub fn names_device(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Prefix(prefix) => {
            matches!(prefix.kind(), Prefix::DeviceNS(_) | Prefix::Verbatim(_))
        }
        Component::Normal(part) => {
            let part = part.to_string_lossy().to_ascii_uppercase();
            let stem = part.split('.').next().unwrap_or("").trim_end();
            let numbered = |name: &str| {
                stem.len() == 4
                    && stem.starts_with(name)
                    && (b'1'..=b'9').contains(&stem.as_bytes()[3])
            };
            matches!(stem, "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
                || numbered("COM")
                || numbered("LPT")
        }
        _ => false,
    })
}

pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    let verbatim = std::fs::canonicalize(path)?;
    let mut components = verbatim.components();
    let ordinary = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::VerbatimDisk(letter) => {
                let mut out = PathBuf::from(format!("{}:\\", letter as char));
                out.extend(components.skip(1)); // skip the root after the prefix
                out
            }
            Prefix::VerbatimUNC(server, share) => {
                let mut out = PathBuf::from(format!(
                    r"\\{}\{}\",
                    server.to_string_lossy(),
                    share.to_string_lossy()
                ));
                out.extend(components.skip(1));
                out
            }
            _ => verbatim.clone(),
        },
        _ => verbatim.clone(),
    };
    Ok(ordinary)
}

/// The key two spellings of one path share. Windows file systems ignore
/// case, so `D:\Repo` and `d:\repo` are one folder; separators are unified
/// too, since git writes `D:/repo`.
pub fn path_key(path: &str) -> String {
    let unified = path.replace('/', "\\");
    unified.trim_end_matches('\\').to_lowercase()
}

/// Accepts connections on one named pipe.
///
/// A named pipe server is a set of pipe instances, one per client. Each
/// instance waits for one client; once a client connects, the next instance
/// must already exist or a second client would find no pipe. So the listener
/// always holds one unconnected instance and creates the next as soon as a
/// client takes it.
pub struct Listener {
    endpoint: String,
    security: SecurityDescriptor,
    next: NamedPipeServer,
}

impl Listener {
    /// Creates the first instance of the pipe.
    ///
    /// When the name is taken, fails with `io::ErrorKind::AddrInUse` if the
    /// owner runs as this user (another Core, most likely), and with
    /// `io::ErrorKind::PermissionDenied` otherwise: someone else holds the
    /// name, and starting anyway would let them receive this user's clients.
    pub async fn bind(endpoint: &str) -> io::Result<Listener> {
        let security = SecurityDescriptor::current_user_only()?;
        let first = match create_instance(endpoint, &security, true) {
            Ok(first) => first,
            // With `first_pipe_instance`, an existing pipe of the same name
            // makes creation fail with ERROR_ACCESS_DENIED, whoever owns it.
            Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
                return Err(match connect(endpoint).await {
                    Ok(_) => io::Error::new(
                        io::ErrorKind::AddrInUse,
                        format!("a process running as this user already serves {endpoint}"),
                    ),
                    Err(e) => io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!("{endpoint} is held by a process that is not this user's ({e})"),
                    ),
                });
            }
            Err(e) => return Err(e),
        };
        Ok(Listener {
            endpoint: endpoint.to_string(),
            security,
            next: first,
        })
    }

    /// Waits for the next client.
    pub async fn accept(&mut self) -> io::Result<BoxStream> {
        let connected = self.next.connect().await;
        // Whether or not the client got through (one that disconnects at once
        // makes `connect` fail), this instance is used up. Put a fresh one in
        // its place first, so the pipe always has an instance listening.
        let fresh = create_instance(&self.endpoint, &self.security, false)?;
        let used = std::mem::replace(&mut self.next, fresh);
        connected?;
        Ok(Box::new(used))
    }
}

fn create_instance(
    endpoint: &str,
    security: &SecurityDescriptor,
    first: bool,
) -> io::Result<NamedPipeServer> {
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        // The Core API is local; remote access goes through an SSH tunnel.
        .reject_remote_clients(true);
    // SAFETY: `attributes` points at a valid security descriptor that
    // `security` keeps alive, and Windows copies what it needs during the call.
    unsafe {
        options.create_with_security_attributes_raw(
            endpoint,
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
        )
    }
}

/// Opens a client connection to `endpoint` and checks that the process
/// serving it runs as the current user, before anything is sent. Retries
/// for a few seconds while every instance is busy.
pub async fn connect(endpoint: &str) -> io::Result<BoxStream> {
    let deadline = tokio::time::Instant::now() + CONNECT_DEADLINE;
    let client = loop {
        let opened = ClientOptions::new()
            // Identification level: the server may learn who we are but can
            // never act as us.
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(endpoint);
        match opened {
            Ok(client) => break client,
            // All instances are taken for a moment, between a client
            // connecting and the listener creating the next instance.
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("{endpoint} stayed busy for {CONNECT_DEADLINE:?}"),
                    ));
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => return Err(e),
        }
    };
    verify_server(&client, endpoint)?;
    Ok(Box::new(client))
}

/// Fails with `PermissionDenied` unless the process serving `client` runs as
/// the current user.
fn verify_server(client: &NamedPipeClient, endpoint: &str) -> io::Result<()> {
    let denied = |why: String| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing {endpoint}: {why}"),
        )
    };
    let pipe = client.as_raw_handle() as HANDLE;
    let mut pid = 0u32;
    // SAFETY: `pipe` is the open client handle `client` owns.
    if unsafe { GetNamedPipeServerProcessId(pipe, &mut pid) } == 0 {
        let e = io::Error::last_os_error();
        return Err(denied(format!("cannot tell which process serves it ({e})")));
    }
    // SAFETY: a plain query; a null result is checked below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        let e = io::Error::last_os_error();
        return Err(denied(cannot_inspect(e)));
    }
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `process` is the handle opened above, closed right after.
    let opened = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) };
    // SAFETY: closed once.
    unsafe { CloseHandle(process) };
    if opened == 0 {
        let e = io::Error::last_os_error();
        return Err(denied(cannot_inspect(e)));
    }
    let server_sid = sid_of_token(token);
    // SAFETY: `token` is the handle opened above.
    unsafe { CloseHandle(token) };
    if server_sid? != current_user_sid()? {
        return Err(denied("it is served by another user's process".into()));
    }
    Ok(())
}

/// Why the serving process could not be inspected. Access denied almost
/// always means it runs elevated (as administrator) while this client does
/// not: an elevated process's token is closed to unelevated processes, even
/// the same user's. The check stays strict; the message says what to do.
fn cannot_inspect(e: io::Error) -> String {
    if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
        "Core is running elevated; run the Shell elevated too, or restart Core normally".into()
    } else {
        format!("cannot read the serving process's user ({e})")
    }
}

/// A security descriptor allocated by Windows, freed on drop.
struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

// SAFETY: the descriptor is never modified after creation, and Windows only
// reads it, so sharing or moving it between threads is fine. Without these,
// the raw pointer would stop `Listener` from moving into a Tokio task.
unsafe impl Send for SecurityDescriptor {}
unsafe impl Sync for SecurityDescriptor {}

impl SecurityDescriptor {
    /// A descriptor whose access list grants full access to the current user
    /// and to nobody else.
    ///
    /// Written in SDDL, Windows' text form for descriptors: `D:` starts the
    /// access list, `P` protects it from inheriting entries, and
    /// `(A;;GA;;;<sid>)` allows generic-all to the account with that SID.
    fn current_user_only() -> io::Result<SecurityDescriptor> {
        let sid = current_user_sid()?;
        let sddl = wide(&format!("D:P(A;;GA;;;{sid})"));
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `sddl` is a NUL-terminated UTF-16 string, and on success
        // Windows writes a LocalAlloc'd descriptor to `descriptor`.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(SecurityDescriptor(descriptor))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: the pointer came from LocalAlloc inside the conversion above
        // and is freed exactly once, here.
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// The current user's SID in its string form (`S-1-5-21-…`), read from this
/// process's access token.
fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing;
    // on success `token` is a real handle, closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = sid_of_token(token);
    // SAFETY: `token` is the handle opened above.
    unsafe { CloseHandle(token) };
    result
}

/// The user SID of an access token, in string form.
fn sid_of_token(token: HANDLE) -> io::Result<String> {
    // First ask how big the TOKEN_USER structure is, then fetch it into a
    // buffer of that size. The first call fails by design.
    let mut size = 0u32;
    // SAFETY: a null buffer with length 0 only asks for the size.
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size) };
    if size == 0 {
        return Err(io::Error::last_os_error());
    }
    // A `u64` buffer rather than `u8`, so it is aligned enough for TOKEN_USER.
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: `buffer` has room for `size` bytes.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: on success the buffer starts with a TOKEN_USER, whose SID
    // pointer points into the same buffer.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };

    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: `sid` is valid while `buffer` lives; on success Windows writes a
    // LocalAlloc'd NUL-terminated string to `text`, freed below.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `text` is NUL-terminated, so scanning to the NUL stays in bounds.
    let string = unsafe {
        let len = (0..).take_while(|&i| *text.add(i) != 0).count();
        String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
    };
    // SAFETY: `text` came from LocalAlloc and is freed once.
    unsafe { LocalFree(text.cast()) };
    Ok(string)
}

/// `s` as a NUL-terminated UTF-16 string, the form Win32 `W` functions take.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// -- Child processes ----------------------------------------------------------

/// A started process and everything it starts in turn, killed as one unit
/// (`ARCH-CORE`, Boundaries: stopped sessions leave no orphans).
///
/// On Windows that unit is a job object. Every process the child starts
/// joins the job too, unless it explicitly breaks away. Made with
/// `kill_on_drop`, the job kills everything in it when its last handle
/// closes. Then dropping a `ProcessTree` kills the tree, and so does Core
/// dying: Windows closes the handle then, which keeps a crashed Core from
/// leaving backends, and the dev servers they started, running with nobody
/// to stop them.
pub struct ProcessTree {
    job: HANDLE,
    kill_on_drop: bool,
}

// SAFETY: a job handle is a kernel handle, usable from any thread; Windows
// serialises the calls made on it.
unsafe impl Send for ProcessTree {}
unsafe impl Sync for ProcessTree {}

impl ProcessTree {
    /// Sets up `command` before it is spawned:
    ///
    /// - Suspended, so it cannot start anything before [`ProcessTree::adopt`]
    ///   has put it in the job and resumed it. A `.cmd` shim starts its real
    ///   program at once, which would otherwise escape the job.
    /// - No console window. Core runs without a console of its own when the
    ///   Shell starts it, and a console program started from such a process
    ///   would otherwise open a window.
    /// - `NoDefaultCurrentDirectoryInExePath`, so the command prompt running
    ///   a `.cmd` shim never looks for the program it calls (`node`, say) in
    ///   the working directory, which is a workspace whose files the user may
    ///   not have written.
    pub fn prepare(command: &mut tokio::process::Command) {
        use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
        command.env("NoDefaultCurrentDirectoryInExePath", "1");
    }

    /// Puts a just-spawned, suspended `child` in a new job, which with
    /// `kill_on_drop` dies with its handle, then lets it run. On an error the
    /// child is still suspended, and the caller kills it.
    pub fn adopt(child: &tokio::process::Child, kill_on_drop: bool) -> io::Result<ProcessTree> {
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        // `None` once the child has been waited on; a child just spawned
        // never has been.
        let process = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("the process exited before it could join a job"))?;
        // SAFETY: both arguments may be null: no inherited handle, no name.
        let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        // Owned from here on, so every early return below closes the handle.
        let tree = ProcessTree { job, kill_on_drop };

        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        if kill_on_drop {
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        }
        // SAFETY: `limits` is the structure this information class expects,
        // and the size passed is its size.
        let ok = unsafe {
            SetInformationJobObject(
                tree.job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast::<c_void>(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `process` is the child's handle, valid while `child` lives,
        // which outlasts this call.
        if unsafe { AssignProcessToJobObject(tree.job, process as HANDLE) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("the process exited before it could be resumed"))?;
        resume_process(pid)?;
        Ok(tree)
    }

    /// Called as soon as the leader has exited: kills the leftovers when the
    /// tree was made with `kill_on_drop`, so a stopped session is gone, tree
    /// and all, by the time its exit is reported. A job handle cannot be
    /// reused, so there is nothing to retire.
    pub fn leader_exited(&self) {
        if self.kill_on_drop {
            let _ = self.kill();
        }
    }

    /// Nothing to retire on Windows: a job handle is never reused.
    pub fn retire(&self) {}

    /// Kills every process in the tree. Killing a tree that has already
    /// ended is not an error.
    pub fn kill(&self) -> io::Result<()> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        // SAFETY: `job` is a valid job handle until `drop`. Exit code 1 is
        // what Windows' own task kill reports.
        if unsafe { TerminateJobObject(self.job, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateJobObjectW and is closed exactly
        // once, here. With kill-on-close set, closing the last handle kills
        // what is left in the job.
        unsafe { CloseHandle(self.job) };
    }
}

/// Resumes every thread of process `pid`, which was created suspended and so
/// has exactly one. Windows has no documented call to resume a process, and
/// the spawned child's thread handle is not kept, so the thread is found by
/// listing the system's threads.
fn resume_process(pid: u32) -> io::Result<()> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    // SAFETY: no pointers; the process id is ignored for a thread snapshot.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    let mut resumed = 0;
    let mut failure = None;
    // SAFETY: `snapshot` is a valid snapshot handle and `entry` has its
    // `dwSize` set, as both calls require.
    let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: plain values in; the handle is checked and closed.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                failure = Some(io::Error::last_os_error());
            } else {
                // SAFETY: `thread` was opened with the right to resume it.
                if unsafe { ResumeThread(thread) } == u32::MAX {
                    failure = Some(io::Error::last_os_error());
                } else {
                    resumed += 1;
                }
                unsafe { CloseHandle(thread) };
            }
        }
        // SAFETY: as for Thread32First.
        more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
    }
    // SAFETY: closed once, here.
    unsafe { CloseHandle(snapshot) };
    match (resumed, failure) {
        (0, Some(e)) => Err(e),
        (0, None) => Err(io::Error::other(format!(
            "process {pid} has no thread to resume"
        ))),
        _ => Ok(()),
    }
}

/// The file Windows should start for `program`.
///
/// Many command-line tools, Claude Code's npm install among them, are `.cmd`
/// shims rather than `.exe` files. Rust's process spawning appends only
/// `.exe` when it searches `PATH`, so `claude` would not be found. This
/// searches `PATH` the way the command prompt does, trying each extension in
/// `PATHEXT`, and returns the full path of the first match. A program given
/// with a directory or an extension, or found nowhere, is returned as is,
/// and spawning reports whatever is wrong with it.
///
/// `path` is the `PATH` the child will get, when the caller overrides it.
pub fn resolve_program(program: &str, path: Option<&str>) -> String {
    let as_given = Path::new(program);
    if as_given.components().count() != 1 || as_given.extension().is_some() {
        return program.to_string();
    }
    let path = match path {
        Some(path) => path.to_string(),
        None => std::env::var("PATH").unwrap_or_default(),
    };
    let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    for dir in path.split(';').filter(|dir| !dir.is_empty()) {
        for extension in extensions.split(';').filter(|ext| !ext.is_empty()) {
            let candidate = Path::new(dir).join(format!("{program}{extension}"));
            if candidate.is_file() {
                return candidate.display().to_string();
            }
        }
    }
    program.to_string()
}
