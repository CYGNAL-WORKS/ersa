//! Typed models of Claude Code's stream-json protocol, as described in
//! `docs/briefs/PROTO-CLAUDE-CODE-STREAM.md` and recorded in the fixtures.
//!
//! [`CliMessage`] is one stdout line; [`HostMessage`] is one stdin line;
//! [`RecordEntry`] is one line of the CLI's session record. Field
//! names follow the CLI's JSON exactly; where the CLI uses camelCase the Rust
//! field is snake_case with a serde rename.
//!
//! Nothing the CLI sends is lost. A message, `system` subtype, content block, or
//! stream event whose kind is not modelled parses into an `Unknown`/`Other`
//! variant holding the raw JSON, and every modelled struct keeps fields it does
//! not name in a flattened `extra` map.

mod content;
mod from_cli;
mod permissions;
mod record;
mod to_cli;

pub use content::*;
pub use from_cli::*;
pub use permissions::*;
pub use record::*;
pub use to_cli::*;
