//! Validation of serialized events against `protocol/schema/agent-events.schema.json`
//! (PROTO-AGENT-EVENTS-R09), and of Core API messages against
//! `protocol/schema/core-api.schema.json` (PROTO-CORE-API-R06). Only compiled
//! with the `schema-validation` feature.
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
//! on each full event, or [`validate_response`] and [`validate_notification`] on
//! what Core writes to the socket.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

use jsonschema::{Registry, Validator};
use serde_json::Value;

/// The schema text, embedded at compile time so tests always check against the
/// file in this checkout, wherever the test binary runs from.
pub const AGENT_EVENTS_SCHEMA: &str =
    include_str!("../../../protocol/schema/agent-events.schema.json");

/// The Core API schema text, embedded the same way.
pub const CORE_API_SCHEMA: &str = include_str!("../../../protocol/schema/core-api.schema.json");

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
        // The Core API schema refers into the agent event schema by URI; the
        // registry answers that URI from the embedded file instead of the network.
        .with_registry(registry())
        .build(&schema)
        .expect("the schemas are valid JSON Schemas")
}

/// A registry holding the agent event schema under its `$id`, so that
/// `agent-events.schema.json#/...` in the Core API schema resolves offline.
fn registry() -> &'static Registry<'static> {
    static REGISTRY: OnceLock<Registry<'static>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let events = schema_json();
        let id = events["$id"]
            .as_str()
            .expect("agent-events.schema.json has an $id")
            .to_string();
        Registry::new()
            .add(id, events)
            .and_then(|builder| builder.prepare())
            .expect("the agent event schema registers")
    })
}

/// Checks `value` against the definition named `def` in the Core API schema,
/// such as `"response"`, `"notification"`, or `"project.add.result"`.
///
/// Panics if `def` is not a definition in the schema, since that is a mistake
/// in the calling test rather than in the value.
pub fn validate_core_api(def: &str, value: &Value) -> Result<(), SchemaViolation> {
    // One validator per definition, built on first use. A `Mutex<HashMap>`
    // rather than a `OnceLock` because the set of definitions is open.
    static VALIDATORS: OnceLock<Mutex<HashMap<String, Arc<Validator>>>> = OnceLock::new();
    let validators = VALIDATORS.get_or_init(Default::default);
    let validator = {
        let mut map = validators.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(def.to_string())
            .or_insert_with(|| {
                let mut schema: Value =
                    serde_json::from_str(CORE_API_SCHEMA).expect("core-api.schema.json is JSON");
                assert!(
                    schema["$defs"].get(def).is_some(),
                    "core-api.schema.json has no definition {def:?}"
                );
                schema["$ref"] = Value::from(format!("#/$defs/{def}"));
                Arc::new(build(schema))
            })
            .clone()
    };
    check(&validator, value)
}

/// Checks a response Core wrote for a request to `method`: the response object
/// itself, and on success its `result` against `<method>.result`.
pub fn validate_response(method: &str, response: &Value) -> Result<(), SchemaViolation> {
    validate_core_api("response", response)?;
    match response.get("result") {
        Some(result) => validate_core_api(&format!("{method}.result"), result),
        None => Ok(()),
    }
}

/// Checks a notification Core wrote.
pub fn validate_notification(notification: &Value) -> Result<(), SchemaViolation> {
    validate_core_api("notification", notification)
}

/// Checks a request, as a client would send it.
pub fn validate_request(request: &Value) -> Result<(), SchemaViolation> {
    validate_core_api("request", request)
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
