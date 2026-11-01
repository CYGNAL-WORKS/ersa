//! Types for the two protocols the Interface speaks to Core: the Core API
//! (requests and responses) and the agent event stream (one stream per session).
//!
//! JSON Schema is the source of truth for both protocols (see `ARCH-CORE`,
//! Implementation decisions). The Rust types here implement those schemas and
//! conformance tests check their serialized form against them.
//!
//! - [`events`]: the agent event stream (`PROTO-AGENT-EVENTS`), implementing
//!   `protocol/schema/agent-events.schema.json`.
//! - `schema` (with the `schema-validation` feature): checks serialized events
//!   and Core API messages against their schemas, for tests in this and other
//!   crates.
//! - [`core_api`]: the Core request/response API (`PROTO-CORE-API`),
//!   implementing `protocol/schema/core-api.schema.json`, with its JSON-RPC
//!   framing in [`core_api::rpc`].
//!
//! This crate must stay free of Core's internals and of any runtime: the
//! Interface's generated TypeScript types and a replay server both depend on
//! exactly what is here, so it is plain data with serde derives and nothing else.

pub mod core_api;
pub mod events;

// `#[cfg(feature = "...")]` compiles the module only when the feature is on.
#[cfg(feature = "schema-validation")]
pub mod schema;
