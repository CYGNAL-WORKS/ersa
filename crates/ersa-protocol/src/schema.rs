//! Validation of serialized events against `protocol/schema/agent-events.schema.json`
//! (PROTO-AGENT-EVENTS-R09). Only compiled with the `schema-validation` feature.
//!
//! Why a feature and not always-on: validation is a test-time concern. Core and
//! the replay server serialize events from the Rust types and never need to
//! re-check them at run time, while the validator pulls in a sizeable dependency
//! tree (regex engines, URL handling). Behind a feature, that tree is compiled
//! only into the test builds that ask for it: a crate that wants to check its
//! output adds
//!
//! ```toml
//! [dev-dependencies]
//! ersa-protocol = { path = "../ersa-protocol", features = ["schema-validation"] }
//! ```
//!
//! and calls [`validate_event`] on each projected payload or [`validate_envelope`]
//! on each full event.

use std::fmt;
use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::Value;

/// The schema text, embedded at compile time so tests always check against the
/// file in this checkout, wherever the test binary runs from.
pub const AGENT_EVENTS_SCHEMA: &str =
    include_str!("../../../protocol/schema/agent-events.schema.json");

/// Why a value did not match the schema: one message per failure, each naming
/// the JSON path of the offending value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaViolation {
    pub messages: Vec<String>,
}

impl fmt::Display for SchemaViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.messages.join("\n"))
    }
}

impl std::error::Error for SchemaViolation {}

/// Checks a whole event (envelope with payload) against the schema.
pub fn validate_envelope(envelope: &Value) -> Result<(), SchemaViolation> {
    // `OnceLock` builds the validator the first time it is needed and hands
    // the same one to every later call, from any thread.
    static VALIDATOR: OnceLock<Validator> = OnceLock::new();
    let validator = VALIDATOR.get_or_init(|| build(schema_json()));
    check(validator, envelope)
}

/// Checks a bare payload (the `event` object, with its `type`) against the
/// schema. This is what an adapter's projector produces, before Core wraps it.
pub fn validate_event(event: &Value) -> Result<(), SchemaViolation> {
    static VALIDATOR: OnceLock<Validator> = OnceLock::new();
    let validator = VALIDATOR.get_or_init(|| {
        // The same document with its root pointed at the payload definition
        // instead of the envelope, so every `$defs` reference still resolves.
        let mut schema = schema_json();
        schema["$ref"] = Value::from("#/$defs/event");
        build(schema)
    });
    check(validator, event)
}

fn schema_json() -> Value {
    serde_json::from_str(AGENT_EVENTS_SCHEMA).expect("agent-events.schema.json is valid JSON")
}

fn build(schema: Value) -> Validator {
    jsonschema::options()
        // In draft 2020-12, `format` is only an annotation unless asked for.
        // Ask, so a `ts` that is not an RFC 3339 date-time fails.
        .should_validate_formats(true)
        .build(&schema)
        .expect("agent-events.schema.json is a valid JSON Schema")
}

fn check(validator: &Validator, value: &Value) -> Result<(), SchemaViolation> {
    let messages: Vec<String> = validator
        .iter_errors(value)
        .map(|e| format!("at '{}': {e}", e.instance_path()))
        .collect();
    if messages.is_empty() {
        Ok(())
    } else {
        Err(SchemaViolation { messages })
    }
}
