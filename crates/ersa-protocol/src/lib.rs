//! Types for the two protocols the Interface speaks to Core: the Core API
//! (requests and responses) and the agent event stream (one stream per session).
//!
//! JSON Schema is the source of truth for both protocols (see `ARCH-CORE`,
//! Implementation decisions). The Rust types here implement those schemas and
//! conformance tests will check their serialized form against them. Until the
//! schemas exist, this crate holds only the protocol version and a placeholder.
//!
//! This crate must stay free of Core's internals and of any runtime: the
//! Interface's generated TypeScript types and a replay server both depend on
//! exactly what is here, so it is plain data with serde derives and nothing else.

use serde::{Deserialize, Serialize};

/// The version of the protocols between the Interface and Core.
///
/// A Shell compares this with the version a Core reports and refuses to attach
/// when they differ (see `PROTO-CORE-API`). It is a plain integer that only
/// ever goes up; there is no notion of "compatible minor versions".
///
/// `#[serde(transparent)]` makes this serialize as the bare number (`1`)
/// rather than as a wrapper object, which is what the wire format wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProtocolVersion(pub u32);

impl ProtocolVersion {
    /// The protocol version this build speaks.
    pub const CURRENT: ProtocolVersion = ProtocolVersion(1);
}

/// What a Core reports about itself when a Shell attaches.
///
/// A placeholder: the real shape is decided by the Core API schema. It exists
/// now so the crate has one serializable type with a round-trip test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionInfo {
    /// The Core binary's version, the workspace version at build time.
    pub core_version: String,
    /// The protocol version that Core speaks.
    pub protocol_version: ProtocolVersion,
}

// Unit tests live next to the code in a `tests` module. `#[cfg(test)]` means
// the module is only compiled for `cargo test`, never into a normal build.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_info_round_trips_through_json() {
        let original = VersionInfo {
            core_version: "0.1.0".to_string(),
            protocol_version: ProtocolVersion::CURRENT,
        };

        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: VersionInfo = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed, original);
    }

    #[test]
    fn protocol_version_serializes_as_a_bare_number() {
        let json = serde_json::to_string(&ProtocolVersion(7)).expect("serialize");
        assert_eq!(json, "7");
    }
}
