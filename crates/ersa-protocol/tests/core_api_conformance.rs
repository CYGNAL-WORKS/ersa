//! Conformance of the Rust Core API types with `protocol/schema/core-api.schema.json`.
//!
//! Examples of every typed result, request, and notification are serialized
//! from the Rust types and validated against the schema file. The negative
//! cases check that the schema stays closed and that `session.event` really
//! follows its reference into the agent event schema.

use std::collections::BTreeSet;

use ersa_protocol::core_api::rpc::{ErrorObject, Notification, Request, RequestId, Response, code};
use ersa_protocol::core_api::*;
use ersa_protocol::events::{
    Answer, AnswerValue, Envelope, Event, Input, PROJECTION_VERSION, PermissionBehavior, Source,
    TurnStarted, TurnTrigger,
};
use ersa_protocol::schema::{
    CORE_API_SCHEMA, validate_core_api, validate_notification, validate_request, validate_response,
};
use serde::Serialize;
use serde_json::{Value, json};

fn schema() -> Value {
    serde_json::from_str(CORE_API_SCHEMA).expect("schema is JSON")
}

fn to_value(value: &impl Serialize) -> Value {
    serde_json::to_value(value).expect("serialize")
}

fn project() -> Project {
    Project {
        id: "p-1a2b3c4d".into(),
        host_id: "local".into(),
        path: "D:/work/ersa".into(),
        name: "ersa".into(),
        vcs: Vcs::Git,
        trusted: false,
    }
}

fn workspace() -> Workspace {
    Workspace {
        id: "w-5e6f7a8b".into(),
        project_id: "p-1a2b3c4d".into(),
        kind: WorkspaceKind::Isolated,
        path: "C:/Users/me/AppData/Local/Ersa/worktrees/ersa/w-5e6f7a8b".into(),
        vcs_ref: "ersa/w-5e6f7a8b".into(),
        state: WorkspaceState::Ready,
    }
}

fn host() -> Host {
    Host {
        id: "local".into(),
        name: "DESKTOP".into(),
        kind: HostKind::Local,
        status: HostStatus::Connected,
        backends: vec![
            BackendStatus {
                name: "claude-code".into(),
                version: Some("2.1.302".into()),
                status: BackendState::Available,
            },
            BackendStatus {
                name: "copilot".into(),
                version: None,
                status: BackendState::Missing,
            },
        ],
    }
}

fn tile() -> Tile {
    Tile {
        state: ersa_protocol::events::LifecycleState::WaitingUser,
        context_tokens: Some(12_000),
        context_window: Some(200_000),
        cost_usd_total: Some(0.42),
        pending: vec![PendingRequest {
            kind: PendingKind::Permission,
            request_id: "r-1".into(),
        }],
    }
}

fn session() -> Session {
    Session {
        id: "s-1".into(),
        workspace_id: "w-5e6f7a8b".into(),
        host_id: "local".into(),
        backend: "claude-code".into(),
        model: Some("opus".into()),
        effort: Some("high".into()),
        permission_mode: "acceptEdits".into(),
        mode: Some("default".into()),
        title: Some("Rename the module".into()),
        state: ersa_protocol::events::LifecycleState::WaitingUser,
        created_at: "2026-10-31T12:00:00Z".into(),
        last_event_seq: Some(41),
        tile: tile(),
    }
}

fn envelope() -> Envelope {
    Envelope {
        v: PROJECTION_VERSION,
        session_id: "s-1".into(),
        seq: 3,
        ts: "2026-10-31T12:00:00.123Z".into(),
        source: Source {
            backend: "claude-code".into(),
            backend_version: "2.1.302".into(),
            input: Input::Live,
            message_id: Some("m-1".into()),
        },
        event: Event::TurnStarted(TurnStarted {
            turn: 1,
            trigger: TurnTrigger::User,
        }),
    }
}

/// One typed result per method that has a Rust result type.
fn result_examples() -> Vec<(Method, Value)> {
    vec![
        (
            Method::CoreHello,
            to_value(&HelloResult {
                core_version: "0.1.0".into(),
                api_version: API_VERSION,
                event_projection_version: PROJECTION_VERSION,
                host: HelloHost {
                    id: "local".into(),
                    name: "DESKTOP".into(),
                    platform: "windows".into(),
                },
                capabilities: vec![],
            }),
        ),
        (
            Method::CoreStatus,
            to_value(&StatusResult {
                sessions_running: 1,
                sessions_waiting: 0,
                uptime_s: 30,
                db_path: "C:/Users/me/AppData/Local/Ersa/ersa.db".into(),
            }),
        ),
        (Method::CoreShutdown, to_value(&Empty {})),
        (Method::HostList, to_value(&vec![host()])),
        (Method::ProjectList, to_value(&vec![project()])),
        (Method::ProjectAdd, to_value(&project())),
        (
            Method::ProjectSetTrust,
            to_value(&Project {
                trusted: true,
                ..project()
            }),
        ),
        (Method::ProjectRemove, to_value(&project())),
        (Method::WorkspaceList, to_value(&vec![workspace()])),
        (Method::WorkspaceCreate, to_value(&workspace())),
        (Method::WorkspaceRemove, to_value(&workspace())),
        (Method::SessionList, to_value(&vec![session()])),
        (Method::SessionCreate, to_value(&session())),
        (
            Method::SessionSend,
            to_value(&SessionSendResult { turn: 2 }),
        ),
        (Method::SessionUnsubscribe, to_value(&Empty {})),
        (
            Method::SessionSubscribe,
            to_value(&SubscribeResult {
                last_seq: Some(41),
                view: "v-1".into(),
                first_seq: Some(7),
            }),
        ),
        (
            Method::SessionSubscribe,
            to_value(&SubscribeResult {
                last_seq: None,
                view: "v-2".into(),
                first_seq: None,
            }),
        ),
        (
            Method::SessionRawLog,
            to_value(&vec![RawLogEntry {
                index: 0,
                ts: "2026-10-31T12:00:00Z".into(),
                dir: RawDirection::Host,
                line: r#"{"type":"user"}"#.into(),
            }]),
        ),
        (
            Method::BackendList,
            to_value(&vec![BackendInfo {
                name: "claude-code".into(),
                label: "Claude Code".into(),
                version: Some("2.1.302".into()),
                status: BackendState::Available,
                tested: TestedRange {
                    min: "2.1.302".into(),
                    max: "2.1.316".into(),
                },
                models: vec![Choice {
                    id: "opus".into(),
                    label: "Opus".into(),
                    default: true,
                }],
                efforts: vec![Choice {
                    id: "medium".into(),
                    label: "Medium".into(),
                    default: true,
                }],
                permission_modes: vec![Choice {
                    id: "default".into(),
                    label: "Ask before acting".into(),
                    default: true,
                }],
            }]),
        ),
        (
            Method::ModeList,
            to_value(&vec![ModeInfo {
                name: "default".into(),
                description: "The backend's own defaults".into(),
            }]),
        ),
        (
            Method::SettingsGet,
            json!({ "workspaces": { "root": "D:/worktrees" } }),
        ),
    ]
}

/// One typed params value per method that has a Rust params type.
fn params_examples() -> Vec<(Method, Value)> {
    vec![
        (
            Method::CoreHello,
            to_value(&HelloParams {
                client: "ersa-shell".into(),
                client_version: "0.1.0".into(),
                api_versions: vec![3],
            }),
        ),
        (Method::CoreStatus, to_value(&Empty {})),
        (
            Method::ProjectAdd,
            to_value(&ProjectAddParams {
                host_id: "local".into(),
                path: "D:/work/ersa".into(),
                trusted: true,
            }),
        ),
        (
            Method::ProjectSetTrust,
            to_value(&ProjectSetTrustParams {
                id: "p-1".into(),
                trusted: false,
            }),
        ),
        (
            Method::ProjectRemove,
            to_value(&IdParams { id: "p-1".into() }),
        ),
        (
            Method::WorkspaceList,
            to_value(&WorkspaceListParams {
                project_id: "p-1".into(),
            }),
        ),
        (
            Method::WorkspaceCreate,
            to_value(&WorkspaceCreateParams {
                project_id: "p-1".into(),
                name: Some("feature/rail".into()),
                base: Some("main".into()),
            }),
        ),
        (
            Method::WorkspaceRemove,
            to_value(&WorkspaceRemoveParams {
                id: "w-1".into(),
                force: Some(true),
            }),
        ),
        (
            Method::SessionCreate,
            to_value(&SessionCreateParams {
                workspace_id: "w-1".into(),
                backend: "claude-code".into(),
                model: Some("opus".into()),
                effort: Some("high".into()),
                permission_mode: None,
                mode: None,
            }),
        ),
        (
            Method::SessionSend,
            to_value(&SessionSendParams {
                id: "s-1".into(),
                text: "hello".into(),
            }),
        ),
        (
            Method::SessionDecidePermission,
            to_value(&DecidePermissionParams {
                id: "s-1".into(),
                request_id: "r-1".into(),
                behavior: PermissionBehavior::Allow,
                message: Some("ok".into()),
                updated_input: Some(
                    json!({ "command": "ls" })
                        .as_object()
                        .cloned()
                        .expect("object"),
                ),
                apply_suggestions: Some(vec![0]),
            }),
        ),
        (
            Method::SessionAnswerQuestion,
            to_value(&AnswerQuestionParams {
                id: "s-1".into(),
                request_id: "q-1".into(),
                answers: vec![
                    Answer {
                        id: "a".into(),
                        value: AnswerValue::One("yes".into()),
                    },
                    Answer {
                        id: "b".into(),
                        value: AnswerValue::Many(vec!["x".into(), "y".into()]),
                    },
                ],
            }),
        ),
        (
            Method::SessionSetTitle,
            to_value(&SetTitleParams {
                id: "s-1".into(),
                title: "t".into(),
            }),
        ),
        (
            Method::SessionSetMode,
            to_value(&SetModeParams {
                id: "s-1".into(),
                mode: "plan".into(),
            }),
        ),
        (
            Method::SessionSubscribe,
            to_value(&SubscribeParams {
                id: "s-1".into(),
                from_seq: 0,
            }),
        ),
        (
            Method::SessionRawLog,
            to_value(&RawLogParams {
                id: "s-1".into(),
                from: Some(0),
                to: None,
            }),
        ),
        (
            Method::BackendList,
            to_value(&BackendListParams {
                host_id: "local".into(),
            }),
        ),
        (
            Method::SettingsSet,
            to_value(&SettingsSetParams {
                patch: json!({ "workspaces": { "root": null } })
                    .as_object()
                    .cloned()
                    .expect("object"),
            }),
        ),
    ]
}

fn notification_examples() -> Vec<Notification> {
    vec![
        Notification::new(
            notification::SESSION_EVENT,
            &SessionEventParams {
                session_id: "s-1".into(),
                event: envelope(),
            },
        )
        .expect("serialize"),
        Notification::new(
            notification::SESSION_TILE,
            &SessionTileParams {
                session_id: "s-1".into(),
                tile: Tile::starting(),
            },
        )
        .expect("serialize"),
        Notification::new(
            notification::HOST_STATUS,
            &HostStatusParams {
                host_id: "local".into(),
                status: HostStatus::Connected,
            },
        )
        .expect("serialize"),
        Notification::new(
            notification::CORE_NOTICE,
            &CoreNoticeParams {
                level: NoticeLevel::Warning,
                message: "disk nearly full".into(),
            },
        )
        .expect("serialize"),
    ]
}

#[test]
fn schema_api_version_matches_the_rust_constant() {
    assert_eq!(schema()["x-api-version"], json!(API_VERSION));
}

#[test]
fn schema_methods_are_exactly_the_rust_methods() {
    let in_schema: BTreeSet<String> = schema()["$defs"]["request"]["oneOf"]
        .as_array()
        .expect("request is a oneOf")
        .iter()
        .map(|branch| {
            branch["properties"]["method"]["const"]
                .as_str()
                .expect("each request branch names its method")
                .to_string()
        })
        .collect();
    let in_rust: BTreeSet<String> = Method::ALL.iter().map(|m| m.name().to_string()).collect();
    assert_eq!(in_schema, in_rust);

    for method in Method::ALL {
        for suffix in ["params", "result"] {
            let def = format!("{}.{suffix}", method.name());
            assert!(schema()["$defs"].get(&def).is_some(), "no {def} definition");
        }
    }
}

// verifies: PROTO-CORE-API-R06
#[test]
fn every_result_example_validates_inside_a_response() {
    for (method, result) in result_examples() {
        let response = to_value(&Response::success(RequestId::Number(1), result));
        if let Err(e) = validate_response(method.name(), &response) {
            panic!(
                "{} result failed the schema:\n{e}\n{response:#}",
                method.name()
            );
        }
    }
}

// verifies: PROTO-CORE-API-R06
#[test]
fn every_notification_example_validates() {
    for notification in notification_examples() {
        let value = to_value(&notification);
        if let Err(e) = validate_notification(&value) {
            panic!("{} failed the schema:\n{e}\n{value:#}", notification.method);
        }
    }
}

#[test]
fn every_params_example_validates_inside_a_request() {
    for (method, params) in params_examples() {
        let request = to_value(&Request::new(
            RequestId::String("a".into()),
            method.name(),
            params,
        ));
        if let Err(e) = validate_request(&request) {
            panic!(
                "{} request failed the schema:\n{e}\n{request:#}",
                method.name()
            );
        }
    }
}

// verifies: PROTO-CORE-API-R06
#[test]
fn error_responses_validate() {
    let cases = [
        Response::failure(
            Some(RequestId::Number(2)),
            ErrorObject::application(
                ErrorKind::InvalidState,
                "host.add is reserved for milestone M4",
            ),
        ),
        Response::failure(
            Some(RequestId::Number(3)),
            ErrorObject::application(ErrorKind::VcsError, "git failed").with_detail("fatal: x"),
        ),
        Response::failure(
            None,
            ErrorObject::protocol(code::PARSE_ERROR, "parse error"),
        ),
    ];
    for response in cases {
        let value = to_value(&response);
        validate_response("core.status", &value).unwrap_or_else(|e| panic!("{e}\n{value:#}"));
    }
}

#[test]
fn an_application_error_without_data_fails() {
    let value = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": { "code": -32000, "message": "no kind" }
    });
    assert!(validate_response("core.status", &value).is_err());
}

#[test]
fn a_response_with_both_result_and_error_fails() {
    let value = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {},
        "error": { "code": -32603, "message": "x" }
    });
    assert!(validate_core_api("response", &value).is_err());
}

#[test]
fn a_result_of_the_wrong_shape_fails() {
    let response = to_value(&Response::success(
        RequestId::Number(1),
        to_value(&workspace()),
    ));
    assert!(validate_response("project.add", &response).is_err());
}

#[test]
fn an_extra_field_on_a_closed_object_fails() {
    let mut value = to_value(&project());
    value["vendor_field"] = json!(1);
    assert!(validate_core_api("project.add.result", &value).is_err());

    let mut value = to_value(&host());
    value["backends"][0]["vendor_field"] = json!(1);
    assert!(validate_core_api("host", &value).is_err());
}

#[test]
fn session_event_follows_its_reference_into_the_agent_event_schema() {
    let mut value = to_value(&notification_examples()[0]);
    validate_notification(&value).expect("the example is valid");
    // An envelope the agent event schema rejects must fail here too, which
    // only happens if the `$ref` was resolved rather than treated as open.
    value["params"]["event"]["v"] = json!(PROJECTION_VERSION + 1);
    assert!(validate_notification(&value).is_err());
}

#[test]
fn reserved_methods_name_a_milestone() {
    let reserved: Vec<_> = Method::ALL
        .iter()
        .filter_map(|m| m.reserved_for().map(|ms| (m.name(), ms)))
        .collect();
    assert!(reserved.contains(&("host.add", "M4")));
    assert!(reserved.contains(&("mode.list", "M3")));
    assert!(reserved.contains(&("terminal.open", "M2")));
    assert!(Method::SessionList.reserved_for().is_none());
}

#[test]
fn typed_values_round_trip() {
    let json = serde_json::to_string(&session()).expect("serialize");
    assert_eq!(
        serde_json::from_str::<Session>(&json).expect("deserialize"),
        session()
    );
    let json = serde_json::to_string(&host()).expect("serialize");
    assert_eq!(
        serde_json::from_str::<Host>(&json).expect("deserialize"),
        host()
    );
}
