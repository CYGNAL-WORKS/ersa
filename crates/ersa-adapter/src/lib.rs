//! The boundary between Core and every adapter (`ARCH-CORE`, Adapters and
//! Transport layer).
//!
//! How the pieces meet, for one session:
//!
//! 1. Core calls [`Adapter::start`] with a [`Transport`] for the session's host,
//!    a [`SessionConfig`], and an [`AdapterSink`] that Core implements.
//! 2. The adapter spawns the backend through the transport and gets a
//!    [`Process`]. It hands every line the backend writes to stdout, and every
//!    line it writes to the backend's stdin, to the sink. Core stamps each one
//!    into a [`RawLine`] and appends it to the session's raw log (ADR 0010).
//! 3. Core runs the adapter's [`Projector`] over the raw log to get Ersa events.
//!    A permission request is just a backend line: projection turns it into
//!    `permission.requested`, the user decides, and Core calls
//!    [`AdapterSession::decide_permission`]. The answer the adapter writes back
//!    is itself a host raw line, so `permission.decided` is projected too.
//!
//! Why `async_trait` rather than Rust's native `async fn` in traits: Core keeps
//! sessions of different backends side by side as `Box<dyn AdapterSession>`, and
//! adapters receive a transport without knowing which one (`Arc<dyn Transport>`).
//! Both need trait objects, and a trait with a native `async fn` cannot be made
//! into one: the size of the future each method returns differs per
//! implementation, so there is no single vtable entry for it. The
//! `#[async_trait]` attribute rewrites each `async fn` to return a boxed future
//! (`Pin<Box<dyn Future + Send>>`), which has one size, at the cost of one small
//! allocation per call. Every call here is a process or I/O operation, so the
//! allocation does not matter.

mod adapter;
mod local;
mod raw;
pub mod recording;
mod transport;

// `#[cfg(feature = "...")]` compiles the module only when the feature is on.
#[cfg(feature = "test-support")]
pub mod fake;

// Re-exported at the crate root so users write `ersa_adapter::Transport`
// rather than reaching into private module paths.
pub use adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, PermissionDecision, Projected, Projector,
    SessionConfig,
};
pub use local::LocalTransport;
pub use raw::{Direction, RawLine};
pub use transport::{LineReader, LineWriter, Process, ProcessExit, ProcessSpec, Transport};

// Adapters implement `#[async_trait]` traits, so they need the attribute too;
// re-exporting it saves each adapter crate a dependency line.
pub use async_trait::async_trait;
