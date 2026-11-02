//! The Core process itself, for Quit (FLOW-CLOSE-AND-QUIT): found from the
//! endpoint it serves, watched until it exits, and ended along with what it
//! started when it will not stop by itself.
//!
//! The Shell may not have started the Core it quits (it usually attached to
//! one an earlier window left running), so the process is found from the
//! endpoint rather than remembered from a launch: the operating system tells
//! a pipe's client which process serves it (Windows) and a socket's client
//! which process is at the other end (Linux).

// The operating system calls below are `unsafe` in Rust; each says why it is
// sound where it is made.
#![allow(unsafe_code)]

use std::io;
use std::time::Duration;

/// How often [`CoreProcess::wait_exit`] looks.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// A running Core, held so it can be waited on and ended. On Windows the
/// process handle is kept open, so its id cannot pass to another process
/// while we hold it.
pub struct CoreProcess {
    pid: u32,
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: a process handle is a kernel handle, usable from any thread.
#[cfg(windows)]
unsafe impl Send for CoreProcess {}
#[cfg(windows)]
unsafe impl Sync for CoreProcess {}

impl CoreProcess {
    /// The process serving `endpoint`, or `None` when nothing does. This
    /// connects and lets go at once, without a request: Core sees a client
    /// that leaves before its handshake.
    ///
    /// This does not check whose process it is; a caller that will end it
    /// checks that first with the verified client (`ersa_core::connect`).
    pub async fn serving(endpoint: &str) -> io::Result<Option<CoreProcess>> {
        imp::serving(endpoint).await
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Whether the process has exited.
    pub fn has_exited(&self) -> bool {
        imp::has_exited(self)
    }

    /// Waits up to `limit` for the process to exit; whether it did.
    pub async fn wait_exit(&self, limit: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + limit;
        loop {
            if self.has_exited() {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Ends the process and every process it started.
    pub fn kill_tree(&self) -> io::Result<()> {
        imp::kill_tree(self)
    }
}

#[cfg(windows)]
impl Drop for CoreProcess {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcess and is closed once, here.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::{ERROR_PIPE_BUSY, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        TerminateProcess, WaitForSingleObject,
    };

    /// How many times to try a pipe whose every instance is busy, a moment apart.
    const BUSY_TRIES: u32 = 100;

    pub async fn serving(endpoint: &str) -> io::Result<Option<CoreProcess>> {
        let mut tries = 0;
        // A plain file open is a pipe client; nothing is written to it.
        let pipe = loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(endpoint)
            {
                Ok(pipe) => break pipe,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e)
                    if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) && tries < BUSY_TRIES =>
                {
                    tries += 1;
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(e) => return Err(e),
            }
        };
        let mut pid = 0u32;
        // SAFETY: `pipe` is an open pipe client handle that outlives the call.
        if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        drop(pipe);
        // SAFETY: a plain open by id; a null result is checked below.
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(CoreProcess { pid, handle }))
    }

    pub fn has_exited(process: &CoreProcess) -> bool {
        // SAFETY: `handle` is open until drop; a zero timeout only asks.
        unsafe { WaitForSingleObject(process.handle, 0) == WAIT_OBJECT_0 }
    }

    pub fn kill_tree(process: &CoreProcess) -> io::Result<()> {
        // Windows' own tree kill walks the processes Core started. Backends
        // sit in job objects that die with Core's handles anyway; this also
        // reaches anything that is not in one. Its output is localised, so
        // only the exit status is read, and Core is ended below regardless.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &process.pid.to_string(), "/T", "/F"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        if has_exited(process) {
            return Ok(());
        }
        // SAFETY: `handle` is open until drop and was opened with
        // PROCESS_TERMINATE. Exit code 1, as Windows' own task kill reports.
        if unsafe { TerminateProcess(process.handle, 1) } == 0 && !has_exited(process) {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(unix)]
mod imp {
    use super::*;

    pub async fn serving(endpoint: &str) -> io::Result<Option<CoreProcess>> {
        let stream = match std::os::unix::net::UnixStream::connect(endpoint) {
            Ok(stream) => stream,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) =>
            {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        Ok(Some(CoreProcess {
            pid: peer_pid(&stream)?,
        }))
    }

    /// The process at the other end of `stream`.
    #[cfg(target_os = "linux")]
    fn peer_pid(stream: &std::os::unix::net::UnixStream) -> io::Result<u32> {
        use std::os::fd::AsRawFd;
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `cred` and `len` describe a buffer of the size this option
        // fills, and the socket is open for the call.
        let ok = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut cred).cast::<libc::c_void>(),
                &mut len,
            )
        };
        if ok != 0 {
            return Err(io::Error::last_os_error());
        }
        u32::try_from(cred.pid).map_err(io::Error::other)
    }

    /// Only Linux is built and tested today; elsewhere Quit asks Core to stop
    /// but cannot end one that will not.
    #[cfg(not(target_os = "linux"))]
    fn peer_pid(_: &std::os::unix::net::UnixStream) -> io::Result<u32> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "finding the process behind a socket is implemented on Linux only",
        ))
    }

    pub fn has_exited(process: &CoreProcess) -> bool {
        // Gone, or a zombie waiting for its parent to reap it, has exited.
        match std::fs::read_to_string(format!("/proc/{}/stat", process.pid)) {
            Ok(stat) => stat
                .rsplit(')')
                .next()
                .is_some_and(|rest| rest.trim_start().starts_with('Z')),
            Err(_) => true,
        }
    }

    pub fn kill_tree(process: &CoreProcess) -> io::Result<()> {
        let pid = libc::pid_t::try_from(process.pid).map_err(io::Error::other)?;
        // The Shell starts Core as the leader of its own process group
        // (src/launch.rs), so the group is Core and what it started that did
        // not make a group of its own.
        // SAFETY: plain system calls on a process id; failures are checked.
        unsafe {
            if libc::getpgid(pid) == pid {
                libc::kill(-pid, libc::SIGKILL);
            }
            if libc::kill(pid, libc::SIGKILL) != 0 && !has_exited(process) {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}
