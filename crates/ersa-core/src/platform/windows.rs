//! Windows: named pipes that only the current user can open.
//!
//! The pipe's security descriptor is the reason for the unsafe code here.
//! Without one, Windows gives a named pipe a default descriptor that lets
//! other accounts on the machine (including anonymous logons, on some
//! configurations) open it for reading. `PROTO-CORE-API` relies on the
//! socket's OS permissions instead of authentication, so the pipe must admit
//! exactly the user Core runs as.

// Calling Win32 functions is `unsafe`; the workspace denies unsafe code
// everywhere except where a platform module opts in, as this one does.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::path::PathBuf;
use std::ptr;
use std::time::Duration;

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};
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
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::BoxStream;

/// `\\.\pipe\ersa-core-<user>`.
pub fn default_endpoint() -> String {
    // A pipe name may contain any character except a backslash, which a
    // Windows user name cannot contain either; replace it anyway to be safe.
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".to_string());
    format!(r"\\.\pipe\ersa-core-{}", user.replace('\\', "_"))
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
    /// Fails with `io::ErrorKind::AddrInUse` when another process already owns
    /// the pipe name, which is how a second Core finds out about the first.
    pub fn bind(endpoint: &str) -> io::Result<Listener> {
        let security = SecurityDescriptor::current_user_only()?;
        let first = create_instance(endpoint, &security, true).map_err(|e| {
            // With `first_pipe_instance`, an existing pipe of the same name
            // makes creation fail with ERROR_ACCESS_DENIED.
            if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
                io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("another process already owns {endpoint}"),
                )
            } else {
                e
            }
        })?;
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

/// Opens a client connection to `endpoint`, waiting while every instance is busy.
pub async fn connect(endpoint: &str) -> io::Result<BoxStream> {
    loop {
        match ClientOptions::new().open(endpoint) {
            Ok(client) => return Ok(Box::new(client)),
            // All instances are taken for a moment, between a client
            // connecting and the listener creating the next instance.
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => return Err(e),
        }
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
