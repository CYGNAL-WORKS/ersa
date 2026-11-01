//! Everything in Core that differs by operating system (ADR 0006). The rest of
//! Core calls only what this module exports, which every platform provides
//! with the same names and signatures:
//!
//! - [`default_endpoint`]: where Core listens: a named pipe on Windows, a Unix
//!   domain socket elsewhere (`PROTO-CORE-API`, Transport and framing).
//!   [`labelled_endpoint`] names another endpoint of the same user's, so a
//!   test or a second Core (`serve --endpoint`) never collides with it.
//! - [`default_data_dir`]: the user's application data directory for Ersa.
//! - [`host_name`]: the machine's name, for the local host.
//! - [`Listener`]: accepts connections on an endpoint, reachable only by the
//!   current user; [`connect`] is the client side, which talks only to an
//!   endpoint the current user serves.
//! - [`canonicalize`] and [`path_key`]: one spelling of a local path, and the
//!   key under which two spellings of the same folder compare equal.
//! - [`ProcessTree`]: a child process and everything it starts, killed as one
//!   unit (a job object on Windows, a process group elsewhere) and started
//!   without a console window; [`resolve_program`] finds the file to start,
//!   Windows `.cmd` shims included.
//!
//! Both are tested in CI: Linux on every push, Windows on changes to Rust.
//! `tests/platform_windows.rs` and `tests/platform_unix.rs` hold the tests
//! that only make sense on one of them.

use tokio::io::{AsyncRead, AsyncWrite};

// `#[cfg(...)]` compiles an item only on the matching target, so exactly one
// of these modules exists in any build.
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::{
    Listener, ProcessTree, canonicalize, connect, default_data_dir, default_endpoint, host_name,
    labelled_endpoint, path_key, resolve_program,
};
#[cfg(windows)]
pub use self::windows::{
    Listener, ProcessTree, canonicalize, connect, default_data_dir, default_endpoint, host_name,
    labelled_endpoint, path_key, resolve_program,
};

/// Checks the label of a [`labelled_endpoint`]: ASCII letters, digits, `-`
/// and `_`, at most 64 of them. It becomes part of a pipe name or a file
/// name, so a separator or `..` must never get in.
fn check_label(label: &str) -> std::io::Result<()> {
    let valid = !label.is_empty()
        && label.len() <= 64
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if valid {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("an endpoint label is letters, digits, '-' and '_', at most 64: {label:?}"),
        ))
    }
}

/// One accepted or opened connection: a byte stream in both directions.
///
/// A trait object so the code above this module never names a platform type.
/// The blanket `impl` makes every type with these traits a `Stream`.
pub trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

/// A boxed [`Stream`].
pub type BoxStream = Box<dyn Stream>;
