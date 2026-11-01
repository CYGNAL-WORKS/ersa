//! Everything in Core that differs by operating system (ADR 0006). The rest of
//! Core calls only what this module exports, which every platform provides
//! with the same names and signatures:
//!
//! - [`default_endpoint`]: where Core listens: a named pipe on Windows, a Unix
//!   domain socket elsewhere (`PROTO-CORE-API`, Transport and framing).
//! - [`default_data_dir`]: the user's application data directory for Ersa.
//! - [`host_name`]: the machine's name, for the local host.
//! - [`Listener`]: accepts connections on an endpoint, reachable only by the
//!   current user; [`connect`] is the client side, which talks only to an
//!   endpoint the current user serves.
//! - [`canonicalize`] and [`path_key`]: one spelling of a local path, and the
//!   key under which two spellings of the same folder compare equal.
//!
//! Windows is built and tested. The Unix implementation compiles but is not
//! tested yet, since CI runs on Windows only.

use tokio::io::{AsyncRead, AsyncWrite};

// `#[cfg(...)]` compiles an item only on the matching target, so exactly one
// of these modules exists in any build.
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::{
    Listener, canonicalize, connect, default_data_dir, default_endpoint, host_name, path_key,
};
#[cfg(windows)]
pub use self::windows::{
    Listener, canonicalize, connect, default_data_dir, default_endpoint, host_name, path_key,
};

/// One accepted or opened connection: a byte stream in both directions.
///
/// A trait object so the code above this module never names a platform type.
/// The blanket `impl` makes every type with these traits a `Stream`.
pub trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

/// A boxed [`Stream`].
pub type BoxStream = Box<dyn Stream>;
