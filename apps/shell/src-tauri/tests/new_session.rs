//! Starting a session from the new-session screen (FLOW-NEW-SESSION) against
//! a real `ersa-core`: the requests the Interface makes (its
//! `session/start.ts`), in the same order, through the Shell's bridge. The
//! backend is the stand-in CLI (examples/stand_in_backend.rs), told to start
//! or to fail, so no test needs Claude Code or a model call.
//!
//! Needs the `ersa-core` executable, as tests/bridge.rs does. Each test's
//! Core serves an endpoint of its own, so the user's own Core is never
//! reached.

mod common;

use std::path::Path;

use common::{finish, git_repo, open, private_location, request, stand_in_backend};
use ersa_shell::bridge::{BridgeError, CoreLink};
use ersa_shell::launch::CoreLocation;
use serde_json::{Value, json};

/// A Core whose Claude Code is the stand-in, started with `mode` set
/// (`ERSA_STAND_IN_ANSWER` or `ERSA_STAND_IN_FAIL`) to `value`.
fn location(dir: &Path, mode: &str, value: &str) -> CoreLocation {
    let mut location = private_location(dir);
    location.env = vec![
        ("ERSA_CLAUDE".into(), stand_in_backend().into_os_string()),
        (mode.into(), value.into()),
        // Nothing the session does may read the user's own Claude home.
        (
            "CLAUDE_CONFIG_DIR".into(),
            dir.join("claude").into_os_string(),
        ),
    ];
    location
}

/// Adds a git repository in `dir` as a project; returns the project.
async fn project(link: &CoreLink, dir: &Path) -> Value {
    let repo = dir.join("repo");
    git_repo(&repo);
    let host = link.hello()["host"]["id"].clone();
    request(
        link,
        "project.add",
        json!({ "host_id": host, "path": repo.display().to_string(), "trusted": false }),
    )
    .await
}

async fn workspaces(link: &CoreLink, project: &Value) -> Vec<Value> {
    let listed = request(
        link,
        "workspace.list",
        json!({ "project_id": project["id"] }),
    )
    .await;
    listed.as_array().expect("a list").clone()
}

async fn sessions(link: &CoreLink) -> Vec<Value> {
    let listed = request(link, "session.list", json!({})).await;
    listed.as_array().expect("a list").clone()
}

/// The default of one of a backend's choices in `backend.list`.
fn default_of(choices: &Value) -> Value {
    choices
        .as_array()
        .expect("choices")
        .iter()
        .find(|c| c["default"] == true)
        .expect("a default")["id"]
        .clone()
}

// verifies: FLOW-NEW-SESSION-R03
// verifies: FLOW-NEW-SESSION-R04
// verifies: FLOW-NEW-SESSION-R12
#[tokio::test]
async fn the_first_message_creates_the_workspace_the_session_and_its_first_turn() {
    let dir = tempfile::tempdir().expect("temp dir");
    let location = location(dir.path(), "ERSA_STAND_IN_ANSWER", "1");
    let checks = tokio::spawn({
        let location = location.clone();
        let dir = dir.path().to_path_buf();
        async move {
            let (link, _how, _events) = open(&location).await;
            let project = project(&link, &dir).await;

            // Opening the screen reads what it offers, and creates nothing.
            let host = link.hello()["host"]["id"].clone();
            let backends = request(&link, "backend.list", json!({ "host_id": host })).await;
            request(&link, "settings.get", json!({})).await;
            let claude = backends
                .as_array()
                .expect("backends")
                .iter()
                .find(|b| b["name"] == "claude-code")
                .expect("Claude Code listed")
                .clone();
            // Detection runs the stand-in (`ERSA_CLAUDE`), which reports
            // one model, the CLI's own `default` entry (PROTO-CORE-API-R72).
            assert_eq!(claude["status"], "available", "{claude}");
            let listed = workspaces(&link, &project).await;
            assert_eq!(listed.len(), 1, "only the root workspace: {listed:?}");
            assert!(sessions(&link).await.is_empty());

            // The first message: a new workspace, the session with the
            // chips' values, and the message as its first turn.
            // The CLI marks no default effort, so none is named.
            let (model, mode) = (
                default_of(&claude["models"]),
                default_of(&claude["permission_modes"]),
            );
            assert_eq!(model, "default");
            let workspace = request(
                &link,
                "workspace.create",
                json!({ "project_id": project["id"] }),
            )
            .await;
            assert_eq!(workspace["kind"], "isolated");
            let session = request(
                &link,
                "session.create",
                json!({
                    "workspace_id": workspace["id"],
                    "backend": "claude-code",
                    "model": model,
                    "permission_mode": mode,
                }),
            )
            .await;
            let sent = request(
                &link,
                "session.send",
                json!({ "id": session["id"], "text": "Fix the build" }),
            )
            .await;
            assert!(sent["turn"].as_u64().is_some(), "a turn: {sent}");

            // The session is listed with a tile, and
            // with the values it started with, for the session controls.
            let listed = sessions(&link).await;
            assert_eq!(listed.len(), 1);
            let row = &listed[0];
            assert_eq!(row["id"], session["id"]);
            assert_eq!(row["workspace_id"], workspace["id"]);
            assert!(row["tile"]["state"].is_string(), "a tile: {row}");
            assert_eq!(
                (&row["model"], row.get("effort"), &row["permission_mode"]),
                (&model, None, &mode)
            );
            assert_eq!(workspaces(&link, &project).await.len(), 2);

            // The values are remembered for the project in Core's settings,
            // as the Interface patches them: every field named, a value the
            // next start leaves out as null, which Core's merge removes.
            let id = project["id"].as_str().expect("an id");
            let entry = |model: Value| {
                json!({ "patch": { "new_session": { "last_used": { id: {
                    "host_id": host, "project_id": id, "workspace": "isolated",
                    "backend": "claude-code", "model": model,
                } } } } })
            };
            request(&link, "settings.set", entry(model.clone())).await;
            let settings = request(&link, "settings.get", json!({})).await;
            assert_eq!(settings["new_session"]["last_used"][id]["model"], model);
            request(&link, "settings.set", entry(Value::Null)).await;
            let settings = request(&link, "settings.get", json!({})).await;
            let remembered = &settings["new_session"]["last_used"][id];
            assert_eq!(remembered["workspace"], "isolated");
            assert!(
                remembered.get("model").is_none(),
                "the earlier model stays: {remembered}"
            );
        }
    });
    finish(checks, &location).await;
}

// verifies: FLOW-NEW-SESSION-R05
#[tokio::test]
async fn a_session_in_the_root_workspace_creates_no_workspace() {
    let dir = tempfile::tempdir().expect("temp dir");
    let location = location(dir.path(), "ERSA_STAND_IN_ANSWER", "1");
    let checks = tokio::spawn({
        let location = location.clone();
        let dir = dir.path().to_path_buf();
        async move {
            let (link, _how, _events) = open(&location).await;
            let project = project(&link, &dir).await;
            let listed = workspaces(&link, &project).await;
            let root = listed
                .iter()
                .find(|w| w["kind"] == "root")
                .expect("the root workspace");
            let session = request(
                &link,
                "session.create",
                json!({ "workspace_id": root["id"], "backend": "claude-code" }),
            )
            .await;
            request(
                &link,
                "session.send",
                json!({ "id": session["id"], "text": "What does this do?" }),
            )
            .await;
            assert_eq!(workspaces(&link, &project).await, listed);
            assert_eq!(sessions(&link).await[0]["workspace_id"], root["id"]);
        }
    });
    finish(checks, &location).await;
}

// verifies: FLOW-NEW-SESSION-R08
#[tokio::test]
async fn a_backend_that_fails_to_start_leaves_no_workspace_and_says_why() {
    const REASON: &str = "stand-in: not logged in";
    let dir = tempfile::tempdir().expect("temp dir");
    let location = location(dir.path(), "ERSA_STAND_IN_FAIL", REASON);
    let checks = tokio::spawn({
        let location = location.clone();
        let dir = dir.path().to_path_buf();
        async move {
            let (link, _how, _events) = open(&location).await;
            let project = project(&link, &dir).await;
            let before = workspaces(&link, &project).await;

            let workspace = request(
                &link,
                "workspace.create",
                json!({ "project_id": project["id"] }),
            )
            .await;
            let session = request(
                &link,
                "session.create",
                json!({ "workspace_id": workspace["id"], "backend": "claude-code" }),
            )
            .await;
            let refused = link
                .request(
                    "session.send",
                    Some(json!({ "id": session["id"], "text": "Fix the build" })),
                )
                .await;
            let Err(BridgeError::Rpc { error }) = refused else {
                panic!("the message should be refused, got {refused:?}");
            };
            assert!(
                error.message.contains(REASON),
                "the backend's own message, got {:?}",
                error.message
            );

            // What the screen does then: the session and the workspace made
            // for it go, in that order.
            let id = json!({ "id": session["id"] });
            request(&link, "session.stop", id.clone()).await;
            request(&link, "session.remove", id).await;
            request(&link, "workspace.remove", json!({ "id": workspace["id"] })).await;
            assert!(sessions(&link).await.is_empty());
            // Removal finishes in the background; the workspace is gone
            // from the list once it has.
            let deadline = std::time::Instant::now() + common::CLEANUP;
            loop {
                let now = workspaces(&link, &project).await;
                if now == before {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the workspace is still listed: {now:?}"
                );
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    });
    finish(checks, &location).await;
}
