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
        waiting: Some(2),
    }
}

fn session() -> Session {
    Session {
        id: "s-1".into(),
        backend_session_id: Some("0f8e2c1a-5b6d-4e7f-8a9b-0c1d2e3f4a5b".into()),
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
        sending: Some(SendPhase::Copying),
        archived: true,
        pinned: true,
        section_id: Some("sec-1".into()),
        unseen: true,
        last_activity_at: Some("2026-10-31T12:05:00.250Z".into()),
        deleting: Some(Deleting::Failed),
        tile: tile(),
    }
}

fn section() -> Section {
    Section {
        id: "sec-1".into(),
        name: "Work".into(),
        created_at: "2026-10-31T12:00:00Z".into(),
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
        (Method::ProjectFind, to_value(&Some(project()))),
        (Method::ProjectFind, to_value(&None::<Project>)),
        (Method::WorkspaceList, to_value(&vec![workspace()])),
        (Method::WorkspaceCreate, to_value(&workspace())),
        (Method::WorkspaceRemove, to_value(&workspace())),
        (
            Method::BranchList,
            to_value(&vec![
                Branch {
                    name: "feature/rail".into(),
                    default: false,
                },
                Branch {
                    name: "main".into(),
                    default: true,
                },
            ]),
        ),
        (
            Method::WorkspaceChanges,
            to_value(&WorkspaceChanges {
                uncommitted: 2,
                ignored: 1,
                unshared: 3,
                stranded: 1,
                sessions: 1,
                unrecorded: false,
                token: "9f2c".into(),
            }),
        ),
        (Method::SessionList, to_value(&vec![session()])),
        (Method::SessionCreate, to_value(&session())),
        (Method::SessionSetArchived, to_value(&session())),
        (Method::SessionFork, to_value(&session())),
        (Method::SessionArrange, to_value(&session())),
        (Method::SectionList, to_value(&vec![section()])),
        (Method::SectionCreate, to_value(&section())),
        (Method::SectionRename, to_value(&section())),
        (Method::SectionRemove, to_value(&section())),
        (
            Method::SessionDelete,
            to_value(&SessionDeleteResult {
                id: "s-1".into(),
                workspace_deleted: true,
                branch_kept: false,
            }),
        ),
        (
            Method::SessionSend,
            to_value(&SessionSendResult { turn: Some(2) }),
        ),
        (
            Method::SessionSend,
            to_value(&SessionSendResult { turn: None }),
        ),
        (
            Method::AttachmentInspect,
            to_value(&Inspected {
                path: "D:/shots/a.png".into(),
                name: "a.png".into(),
                kind: AttachmentKind::Image,
                size_bytes: Some(1234),
                lines: None,
                media_type: Some("image/png".into()),
                target: Some("D:/elsewhere/a.png".into()),
                copied: false,
            }),
        ),
        (
            Method::AttachmentInspect,
            to_value(&Inspected {
                path: "D:/notes/a.md".into(),
                name: "a.md".into(),
                kind: AttachmentKind::File,
                size_bytes: Some(12),
                lines: Some(1),
                media_type: None,
                target: None,
                copied: true,
            }),
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
                models: vec![
                    ModelChoice {
                        id: "opus".into(),
                        label: "Opus".into(),
                        default: true,
                        resolved: Some("claude-opus-5-5".into()),
                        efforts: Some(vec!["low".into(), "medium".into(), "high".into()]),
                    },
                    // A model that takes no effort and reports no resolved id.
                    ModelChoice {
                        id: "haiku".into(),
                        label: "Haiku".into(),
                        default: false,
                        resolved: None,
                        efforts: None,
                    },
                ],
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
                api_versions: vec![API_VERSION],
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
            Method::BranchList,
            to_value(&BranchListParams {
                project_id: "p-1".into(),
            }),
        ),
        (
            Method::WorkspaceChanges,
            to_value(&IdParams { id: "w-1".into() }),
        ),
        (
            Method::ProjectFind,
            to_value(&ProjectFindParams {
                host_id: "local".into(),
                path: "D:/code/ersa".into(),
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
                attachments: Vec::new(),
            }),
        ),
        (
            Method::SessionSend,
            to_value(&SessionSendParams {
                id: "s-1".into(),
                text: String::new(),
                attachments: vec![
                    Attachment {
                        kind: AttachmentKind::Folder,
                        path: Some("D:/work/notes".into()),
                        media_type: None,
                        data: None,
                    },
                    Attachment {
                        kind: AttachmentKind::Image,
                        path: None,
                        media_type: Some("image/png".into()),
                        data: Some("iVBORw0KGgo=".into()),
                    },
                ],
            }),
        ),
        (
            Method::AttachmentInspect,
            to_value(&InspectParams {
                path: "D:/shots/a.png".into(),
                workspace_id: Some("w-1".into()),
                host_id: None,
            }),
        ),
        (
            Method::AttachmentInspect,
            to_value(&InspectParams {
                path: "D:/shots/a.png".into(),
                workspace_id: None,
                host_id: Some("local".into()),
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
                        own_words: true,
                    },
                    Answer {
                        id: "b".into(),
                        value: AnswerValue::Many(vec!["x".into(), "y".into()]),
                        own_words: false,
                    },
                ],
                declined: Some(false),
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
            Method::SessionFork,
            to_value(&IdParams { id: "s-1".into() }),
        ),
        (
            Method::SessionSetArchived,
            to_value(&SetArchivedParams {
                id: "s-1".into(),
                archived: true,
            }),
        ),
        (
            Method::SessionArrange,
            to_value(&ArrangeParams {
                id: "s-1".into(),
                pinned: Some(true),
                section_id: Some(Some("sec-1".into())),
                unseen: Some(false),
            }),
        ),
        (
            Method::SessionArrange,
            to_value(&ArrangeParams {
                id: "s-1".into(),
                pinned: None,
                section_id: Some(None),
                unseen: None,
            }),
        ),
        (Method::SectionList, to_value(&Empty {})),
        (
            Method::SectionCreate,
            to_value(&SectionCreateParams {
                name: "Work".into(),
            }),
        ),
        (
            Method::SectionRename,
            to_value(&SectionRenameParams {
                id: "sec-1".into(),
                name: "Later".into(),
            }),
        ),
        (
            Method::SectionRemove,
            to_value(&IdParams { id: "sec-1".into() }),
        ),
        (
            Method::SessionDelete,
            to_value(&SessionDeleteParams {
                id: "s-1".into(),
                token: Some("9f2c".into()),
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
                last_activity_at: Some("2026-10-31T12:05:00.250Z".into()),
                title: Some("Rename the module".into()),
            },
        )
        .expect("serialize"),
        Notification::new(
            notification::SESSION_SEND_PROGRESS,
            &SendProgressParams {
                session_id: "s-1".into(),
                phase: SendPhase::Copying,
                copied_bytes: Some(10),
                total_bytes: Some(20),
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
            Some(RequestId::Number(4)),
            ErrorObject::application(ErrorKind::AttachmentRefused, "nothing at the path")
                .with_index(1),
        ),
        Response::failure(
            Some(RequestId::Number(5)),
            ErrorObject::application(ErrorKind::DeliveryUnknown, "the backend stopped"),
        ),
        Response::failure(
            Some(RequestId::Number(6)),
            ErrorObject::application(ErrorKind::InvalidState, "the folder is gone")
                .with_reason(ErrorReason::FolderGone),
        ),
        Response::failure(
            Some(RequestId::Number(7)),
            ErrorObject::application(ErrorKind::InvalidState, "open in another process")
                .with_reason(ErrorReason::OpenElsewhere)
                .with_detail("claude (pid 4242)"),
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

/// Every reason the brief names serializes to a value the schema takes, and
/// the schema takes no other, `resume_not_built` (replaced by `folder_gone`)
/// included.
#[test]
fn error_reasons_are_exactly_the_schemas() {
    use ErrorReason::*;
    let all = [
        SessionBusy,
        SessionDeleting,
        SessionArchived,
        SessionNotRunning,
        SendInProgress,
        SendStopped,
        FolderGone,
        BackendUnnamed,
        RecordGone,
        OpenElsewhere,
        OpenUnknown,
        NoFinishedReply,
        BackendCannotFork,
    ];
    let ours: BTreeSet<String> = all.iter().map(|r| to_value(r).to_string()).collect();
    let theirs: BTreeSet<String> = schema()["$defs"]["error_data"]["properties"]["reason"]["enum"]
        .as_array()
        .expect("reason is an enum")
        .iter()
        .map(Value::to_string)
        .collect();
    assert_eq!(ours, theirs);
    let mut data = json!({ "kind": "invalid_state", "reason": "resume_not_built" });
    assert!(validate_core_api("error_data", &data).is_err());
    data["reason"] = json!("folder_gone");
    validate_core_api("error_data", &data).expect("folder_gone is a reason");
}

/// The new shapes stay closed: what only a model has is refused on an
/// effort, a tile never says zero messages wait, and a session without
/// `unseen` is incomplete.
#[test]
fn track_a_shapes_stay_closed() {
    let backends = to_value(
        &result_examples()
            .into_iter()
            .find(|(m, _)| *m == Method::BackendList)
            .expect("a backend.list example")
            .1,
    );
    validate_core_api("backend.list.result", &backends).expect("the example is valid");
    let mut effort_with_resolved = backends.clone();
    effort_with_resolved[0]["efforts"][0]["resolved"] = json!("x");
    assert!(validate_core_api("backend.list.result", &effort_with_resolved).is_err());
    let mut model_with_stray = backends;
    model_with_stray[0]["models"][0]["vendor_field"] = json!(1);
    assert!(validate_core_api("backend.list.result", &model_with_stray).is_err());

    let mut tile = to_value(&tile());
    validate_core_api("tile", &tile).expect("the example is valid");
    tile["waiting"] = json!(0);
    assert!(validate_core_api("tile", &tile).is_err());

    let mut session = to_value(&session());
    session.as_object_mut().unwrap().remove("unseen");
    assert!(validate_core_api("session", &session).is_err());

    let mut arrange = json!({ "id": "s-1", "unseen": true });
    validate_core_api("session.arrange.params", &arrange).expect("unseen is arrangeable");
    arrange["unseen"] = json!("yes");
    assert!(validate_core_api("session.arrange.params", &arrange).is_err());
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

/// `session.arrange`'s `section_id` has three meanings: left out leaves the
/// section, `null` is no section, and an id is that section.
#[test]
fn arrange_params_tell_a_missing_section_from_a_null_one() {
    let parse = |value: Value| serde_json::from_value::<ArrangeParams>(value).expect("parses");
    assert_eq!(parse(json!({ "id": "s-1" })).section_id, None);
    assert_eq!(
        parse(json!({ "id": "s-1", "section_id": null })).section_id,
        Some(None)
    );
    assert_eq!(
        parse(json!({ "id": "s-1", "section_id": "sec-1" })).section_id,
        Some(Some("sec-1".to_string()))
    );
    let null = to_value(&ArrangeParams {
        id: "s-1".into(),
        pinned: None,
        section_id: Some(None),
        unseen: None,
    });
    assert_eq!(null, json!({ "id": "s-1", "section_id": null }));
}
