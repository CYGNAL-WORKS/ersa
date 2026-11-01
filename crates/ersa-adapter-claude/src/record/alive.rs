//! Whether a process is still running, for the live-session registry.
//!
//! Each platform asks its own kernel: nothing here starts a process or reads
//! another program's output.

/// Whether process `pid` is running. A process that exists but belongs to
/// someone Ersa may not inspect still counts as running.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn is_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, GetLastError, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    // Pid 0 is the System Idle Process, never a CLI.
    if pid == 0 {
        return false;
    }
    // SAFETY: a plain query; a null result is checked below.
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if process.is_null() {
        // No such process gives ERROR_INVALID_PARAMETER. Access denied means
        // it exists (elevated, or another user's).
        // SAFETY: reads this thread's last error, set by the call above.
        return unsafe { GetLastError() } == ERROR_ACCESS_DENIED;
    }
    // A process object outlives its process while handles to it are open,
    // so opening it is not enough: it is running until it is signalled.
    // SAFETY: `process` is the handle opened above, with SYNCHRONIZE access.
    let running = unsafe { WaitForSingleObject(process, 0) } == WAIT_TIMEOUT;
    // SAFETY: closed once.
    unsafe { CloseHandle(process) };
    running
}

/// Whether process `pid` is running. A process that exists but belongs to
/// someone Ersa may not signal still counts as running.
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn is_running(pid: u32) -> bool {
    // Pid 0 would signal this process group, and a pid above i32::MAX would
    // wrap to a negative one, which signals a group; neither names a process.
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // Signal 0 checks that the process exists without sending anything.
    // SAFETY: `kill` with signal 0 has no effect beyond the check.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_running() {
        assert!(is_running(std::process::id()));
    }

    #[test]
    fn an_exited_child_is_not() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!is_running(pid));
    }

    #[test]
    fn pids_that_name_no_process_are_not() {
        assert!(!is_running(0));
        assert!(!is_running(u32::MAX));
    }
}
