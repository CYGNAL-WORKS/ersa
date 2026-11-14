//! JSON-RPC 2.0 framing for the Core API: the request, response, notification,
//! and error objects every line on the socket is made of.
//!
//! Hand-rolled with serde rather than taken from a JSON-RPC crate: the subset
//! Ersa uses (one message per line, no batches) is a few dozen lines, and both
//! Core and the replay server need exactly this behaviour.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ErrorKind, ErrorReason};

/// The `jsonrpc` member every message carries.
pub const JSONRPC_VERSION: &str = "2.0";

/// Standard JSON-RPC codes for protocol-level failures, and the one code
/// `PROTO-CORE-API` uses for application errors.
pub mod code {
    /// The line was not valid JSON.
    pub const PARSE_ERROR: i64 = -32700;
    /// Valid JSON, but not a JSON-RPC request object.
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// The method exists but its `params` do not match the schema.
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
    /// Every application error; `data.kind` says which.
    pub const APPLICATION_ERROR: i64 = -32000;
}

/// A request's `id`, which the response echoes. JSON-RPC allows a string or a
/// number; `untagged` makes serde accept either and write it back unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}

/// A request, from the Interface to Core.
///
/// `params` stays raw JSON here; the handler for `method` parses it into that
/// method's params type, so a bad `params` gets `invalid params` and not
/// `invalid request`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Request {
    /// A request with the right `jsonrpc` member.
    pub fn new(id: RequestId, method: &str, params: Value) -> Self {
        Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            method: method.to_string(),
            params: Some(params),
        }
    }
}

/// Core's answer to one request.
///
/// `id` is `None` (written as `null`) only when the request was so malformed
/// that its id could not be read; JSON-RPC requires the member even then.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Option<RequestId>,
    /// `flatten` puts the enum's single key (`result` or `error`) directly on
    /// the response object instead of nesting it.
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// A response carries exactly one of these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Result(Value),
    Error(ErrorObject),
}

impl Response {
    pub fn success(id: RequestId, result: Value) -> Self {
        Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(id),
            outcome: Outcome::Result(result),
        }
    }

    pub fn failure(id: Option<RequestId>, error: ErrorObject) -> Self {
        Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            outcome: Outcome::Error(error),
        }
    }
}

/// A notification: a message with no `id`, which gets no response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
}

impl Notification {
    /// A notification whose params are `params` serialized. The notification
    /// params types in this crate always serialize, so this cannot fail for them.
    pub fn new(method: &str, params: &impl Serialize) -> Result<Self, serde_json::Error> {
        Ok(Notification {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.to_string(),
            params: serde_json::to_value(params)?,
        })
    }
}

/// A JSON-RPC error object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorObject {
    pub code: i64,
    pub message: String,
    /// Always present on application errors (`code` -32000); absent on
    /// protocol-level ones, whose code already says what went wrong.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<ErrorData>,
}

/// What kind of application error, so the Interface can react without parsing
/// the message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorData {
    pub kind: ErrorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// With `attachment_refused`: the refused attachment's place in
    /// `session.send`'s `attachments`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    /// With `invalid_state`: which refusal the user met, when the brief
    /// names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<ErrorReason>,
}

impl ErrorObject {
    /// An application error (`-32000`) of the given kind.
    pub fn application(kind: ErrorKind, message: impl Into<String>) -> Self {
        ErrorObject {
            code: code::APPLICATION_ERROR,
            message: message.into(),
            data: Some(ErrorData {
                kind,
                detail: None,
                index: None,
                reason: None,
            }),
        }
    }

    /// A protocol-level error with one of the standard codes.
    pub fn protocol(code: i64, message: impl Into<String>) -> Self {
        ErrorObject {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// The same error with `data.detail` set. Does nothing to a protocol-level
    /// error, which has no `data`.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        if let Some(data) = &mut self.data {
            data.detail = Some(detail.into());
        }
        self
    }

    /// The same error with `data.index` set. Does nothing to a
    /// protocol-level error, which has no `data`.
    pub fn with_index(mut self, index: u32) -> Self {
        if let Some(data) = &mut self.data {
            data.index = Some(index);
        }
        self
    }

    /// The same error with `data.reason` set. Does nothing to a
    /// protocol-level error, which has no `data`.
    pub fn with_reason(mut self, reason: ErrorReason) -> Self {
        if let Some(data) = &mut self.data {
            data.reason = Some(reason);
        }
        self
    }

    /// The error's kind, for application errors.
    pub fn kind(&self) -> Option<ErrorKind> {
        self.data.as_ref().map(|d| d.kind)
    }
}

/// One line read from the socket, classified.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Request(Request),
    /// A message without an `id`. Nothing is answered.
    Notification {
        method: String,
        params: Option<Value>,
    },
    /// Not a usable message. Answer with `error`, using `id` when it could be
    /// read.
    Invalid {
        id: Option<RequestId>,
        error: ErrorObject,
    },
}

/// Classifies one line, without its terminator.
///
/// Batches (a JSON array of requests) are valid JSON-RPC but `PROTO-CORE-API`
/// has no use for them, so they are rejected as invalid requests.
pub fn parse_incoming(line: &str) -> Incoming {
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(e) => {
            return Incoming::Invalid {
                id: None,
                error: ErrorObject::protocol(code::PARSE_ERROR, format!("parse error: {e}")),
            };
        }
    };
    let invalid = |id: Option<RequestId>, message: &str| Incoming::Invalid {
        id,
        error: ErrorObject::protocol(code::INVALID_REQUEST, message),
    };

    let Value::Object(mut object) = value else {
        return invalid(None, "a message must be a JSON object");
    };
    // Read the id first so every later failure can echo it.
    let id = match object.remove("id") {
        None => None,
        Some(raw) => match serde_json::from_value::<RequestId>(raw) {
            Ok(id) => Some(id),
            Err(_) => return invalid(None, "id must be a string or an integer"),
        },
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some(JSONRPC_VERSION) {
        return invalid(id, "jsonrpc must be \"2.0\"");
    }
    let Some(Value::String(method)) = object.remove("method") else {
        return invalid(id, "method must be a string");
    };
    let params = object.remove("params");
    match id {
        Some(id) => Incoming::Request(Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            method,
            params,
        }),
        None => Incoming::Notification { method, params },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_success_response_has_result_and_no_error() {
        let response = Response::success(RequestId::Number(3), json!({ "a": 1 }));
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({ "jsonrpc": "2.0", "id": 3, "result": { "a": 1 } })
        );
    }

    #[test]
    fn an_error_without_an_id_writes_null() {
        let error = ErrorObject::protocol(code::PARSE_ERROR, "bad");
        let response = Response::failure(None, error);
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "bad" } })
        );
    }

    #[test]
    fn responses_round_trip() {
        let error =
            ErrorObject::application(ErrorKind::NotFound, "no such project").with_detail("p-1");
        let response = Response::failure(Some(RequestId::String("x".into())), error);
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&json).unwrap(), response);
    }

    #[test]
    fn lines_are_classified() {
        assert!(matches!(
            parse_incoming(r#"{"jsonrpc":"2.0","id":1,"method":"core.status"}"#),
            Incoming::Request(Request {
                id: RequestId::Number(1),
                ..
            })
        ));
        assert!(matches!(
            parse_incoming(r#"{"jsonrpc":"2.0","method":"x.y"}"#),
            Incoming::Notification { .. }
        ));
        let Incoming::Invalid { id, error } = parse_incoming("{not json") else {
            panic!("expected invalid");
        };
        assert_eq!((id, error.code), (None, code::PARSE_ERROR));
        let Incoming::Invalid { id, error } =
            parse_incoming(r#"{"jsonrpc":"1.0","id":"a","method":"m"}"#)
        else {
            panic!("expected invalid");
        };
        assert_eq!(
            (id, error.code),
            (Some(RequestId::String("a".into())), code::INVALID_REQUEST)
        );
        assert!(matches!(parse_incoming("[]"), Incoming::Invalid { .. }));
    }
}
