//! Attachments on `session.send`, the session's scratch folder, and
//! `attachment.inspect`, driven through the Core API over the real socket
//! with the fake adapter (`support/fake.rs`) standing in for a backend: it
//! records every message as Core handed it over. Each test works in
//! temporary folders: the session's workspace, and a folder outside it.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ersa_adapter::OutgoingAttachment;
use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, Stats};
use support::{Client, TestCore, git_repo, kind, start_with_adapters, tempdir};

/// The first bytes of a PNG, and of a GIF: enough to tell each by.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
const GIF: &[u8] = b"GIF89a\x01\0\x01\0\0\0\0;";

struct Setup {
    core: TestCore,
    stats: Arc<Stats>,
    client: Client,
    workspace_id: String,
    /// The session's workspace.
    workspace: PathBuf,
    /// A folder outside the workspace.
    outside: PathBuf,
}

async fn setup(adapter: FakeAdapter) -> Setup {
    let stats = Arc::clone(&adapter.stats);
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let project = client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": repo.display().to_string(), "trusted": false }),
        )
        .await;
    // The root workspace: the project's own folder.
    let workspaces = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    let root = &workspaces[0];
    let outside = core.dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    Setup {
        workspace_id: root["id"].as_str().unwrap().to_string(),
        workspace: PathBuf::from(root["path"].as_str().unwrap()),
        outside,
        core,
        stats,
        client,
    }
}

impl Setup {
    async fn session(&mut self) -> String {
        let session = self
            .client
            .result(
                "session.create",
                json!({ "workspace_id": self.workspace_id, "backend": fake::BACKEND }),
            )
            .await;
        session["id"].as_str().unwrap().to_string()
    }

    async fn send(&mut self, id: &str, text: &str, attachments: Value) -> Value {
        self.client
            .call(
                "session.send",
                json!({ "id": id, "text": text, "attachments": attachments }),
            )
            .await
    }

    fn messages(&self) -> Vec<ersa_adapter::OutgoingMessage> {
        self.stats.messages.lock().unwrap().clone()
    }

    fn scratch(&self, id: &str) -> PathBuf {
        self.workspace.join(".ersa").join("attachments").join(id)
    }
}

fn path(p: &Path) -> String {
    p.display().to_string()
}

fn file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

/// The numbered copies in a session's scratch folder, `.partial` aside.
fn copies(scratch: &Path) -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(scratch) {
        Ok(entries) => entries
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != ".partial")
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

fn refused(response: &Value, index: u64) -> String {
    let error = &response["error"];
    assert_eq!(kind(error), "attachment_refused", "{response:#}");
    assert_eq!(error["data"]["index"], index, "{response:#}");
    error["message"].as_str().unwrap().to_string()
}

// verifies: PROTO-CORE-API-R21
#[tokio::test]
async fn images_go_inline_and_paths_inside_the_workspace_go_relative() {
    let mut s = setup(FakeAdapter::new(1)).await;
    let id = s.session().await;
    let png = file(&s.outside, "shot.png", PNG);
    let inside = file(&s.workspace, "src/a.txt", b"hello\n");
    std::fs::create_dir_all(s.workspace.join("docs")).unwrap();
    let response = s
        .send(
            &id,
            "look",
            json!([
                { "kind": "image", "path": path(&png) },
                { "kind": "image", "media_type": "image/gif", "data": BASE64.encode(GIF) },
                { "kind": "file", "path": path(&inside) },
                { "kind": "folder", "path": path(&s.workspace.join("docs")) },
            ]),
        )
        .await;
    assert!(
        response["result"]["turn"].as_u64().unwrap() >= 1,
        "{response:#}"
    );
    let messages = s.messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "look");
    assert_eq!(
        messages[0].attachments,
        vec![
            OutgoingAttachment::Image {
                media_type: "image/png".into(),
                data: BASE64.encode(PNG),
            },
            OutgoingAttachment::Image {
                media_type: "image/gif".into(),
                data: BASE64.encode(GIF),
            },
            OutgoingAttachment::File {
                path: "src/a.txt".into()
            },
            OutgoingAttachment::Folder {
                path: "docs".into()
            },
        ]
    );
    // Nothing was copied: no scratch folder at all.
    assert!(!s.workspace.join(".ersa").exists());
}

// verifies: PROTO-CORE-API-R22
#[tokio::test]
async fn attachments_from_outside_are_copied_into_the_sessions_own_scratch_folder() {
    let mut s = setup(FakeAdapter::new(1)).await;
    let first = s.session().await;
    let second = s.session().await;
    let notes = file(&s.outside, "notes.md", b"# notes\n");
    file(&s.outside, "dir/a.txt", b"a");
    file(&s.outside, "dir/sub/b.txt", b"bb");

    let response = s
        .send(
            &first,
            "",
            json!([
                { "kind": "file", "path": path(&notes) },
                { "kind": "folder", "path": path(&s.outside.join("dir")) },
            ]),
        )
        .await;
    assert!(response.get("result").is_some(), "{response:#}");
    let sent = &s.messages()[0].attachments;
    assert_eq!(
        sent,
        &vec![
            OutgoingAttachment::File {
                path: format!(".ersa/attachments/{first}/1/notes.md")
            },
            OutgoingAttachment::Folder {
                path: format!(".ersa/attachments/{first}/2/dir")
            },
        ]
    );
    let scratch = s.scratch(&first);
    assert_eq!(
        std::fs::read(scratch.join("1").join("notes.md")).unwrap(),
        b"# notes\n"
    );
    assert_eq!(
        std::fs::read(scratch.join("2").join("dir").join("sub").join("b.txt")).unwrap(),
        b"bb"
    );
    // `.ersa/` is ignored by version control.
    assert_eq!(
        std::fs::read_to_string(s.workspace.join(".ersa").join(".gitignore")).unwrap(),
        "*\n"
    );

    // The next send numbers on, and the first message's copies stay.
    s.send(
        &first,
        "",
        json!([{ "kind": "file", "path": path(&notes) }]),
    )
    .await;
    assert_eq!(copies(&scratch), vec!["1", "2", "3"]);

    // Another session in the same workspace has its own folder and numbers.
    s.send(
        &second,
        "",
        json!([{ "kind": "file", "path": path(&notes) }]),
    )
    .await;
    assert_eq!(
        s.messages()[2].attachments,
        vec![OutgoingAttachment::File {
            path: format!(".ersa/attachments/{second}/1/notes.md")
        }]
    );
    assert_eq!(copies(&scratch), vec!["1", "2", "3"]);
}

// verifies: PROTO-CORE-API-R22
#[tokio::test]
async fn numbering_continues_past_the_highest_copy_already_there() {
    let mut s = setup(FakeAdapter::new(1)).await;
    let id = s.session().await;
    // As a Core before a restart would have left it.
    std::fs::create_dir_all(s.scratch(&id).join("7")).unwrap();
    let notes = file(&s.outside, "notes.md", b"x");
    s.send(&id, "", json!([{ "kind": "file", "path": path(&notes) }]))
        .await;
    assert_eq!(
        s.messages()[0].attachments,
        vec![OutgoingAttachment::File {
            path: format!(".ersa/attachments/{id}/8/notes.md")
        }]
    );
}

// verifies: PROTO-CORE-API-R22
#[cfg(unix)]
#[tokio::test]
async fn a_copied_folder_leaves_its_links_out() {
    let mut s = setup(FakeAdapter::new(1)).await;
    let id = s.session().await;
    let dir = s.outside.join("dir");
    file(&dir, "kept.txt", b"k");
    let secret = file(&s.outside, "secret.txt", b"s");
    std::os::unix::fs::symlink(&secret, dir.join("link.txt")).unwrap();
    s.send(&id, "", json!([{ "kind": "folder", "path": path(&dir) }]))
        .await;
    let copied = s.scratch(&id).join("1").join("dir");
    assert!(copied.join("kept.txt").exists());
    assert!(std::fs::symlink_metadata(copied.join("link.txt")).is_err());
}

// verifies: PROTO-CORE-API-R23
#[tokio::test]
async fn an_attachment_that_cant_be_sent_refuses_the_whole_message() {
    let mut s = setup(FakeAdapter::new(1)).await;
    let id = s.session().await;
    let notes = path(&file(&s.outside, "notes.md", b"x"));
    let text = path(&file(&s.outside, "a.txt", b"not an image"));
    let big = path(&file(&s.outside, "big.png", &[PNG, &[0u8; 2048]].concat()));
    let gone = path(&s.outside.join("gone.txt"));
    // The workspace's parent holds the workspace.
    let parent = path(s.workspace.parent().unwrap());
    let cases = [
        (json!({ "kind": "file", "path": gone }), "nothing is at"),
        (
            json!({ "kind": "file", "path": format!("{notes}\u{7}") }),
            "control character",
        ),
        (json!({ "kind": "image", "path": text }), "not an image"),
        (json!({ "kind": "image", "path": big }), "larger"),
        (
            json!({ "kind": "image", "media_type": "image/png", "data": BASE64.encode(GIF) }),
            "image/png",
        ),
        (
            json!({ "kind": "folder", "path": parent }),
            "holds the session's workspace",
        ),
    ];
    for (bad, why) in cases {
        // The good one first: the bad one's place is named.
        let response = s
            .send(&id, "hi", json!([{ "kind": "file", "path": notes }, bad]))
            .await;
        let message = refused(&response, 1);
        assert!(message.contains(why), "{why}: {message}");
    }
    // Nothing was sent, and nothing copied stays.
    assert!(s.messages().is_empty());
    assert_eq!(copies(&s.scratch(&id)), Vec::<String>::new());
}

// verifies: PROTO-CORE-API-R23
#[tokio::test]
async fn a_scratch_folder_that_is_not_a_real_folder_refuses_the_copy() {
    let mut s = setup(FakeAdapter::new(1)).await;
    let id = s.session().await;
    let notes = path(&file(&s.outside, "notes.md", b"x"));
    // `.ersa/.gitignore` must be a plain file.
    std::fs::create_dir_all(s.workspace.join(".ersa").join(".gitignore")).unwrap();
    let response = s
        .send(&id, "", json!([{ "kind": "file", "path": notes }]))
        .await;
    let message = refused(&response, 0);
    assert!(message.contains(".gitignore"), "{message}");
    assert!(s.messages().is_empty());
    assert!(!s.workspace.join(".ersa").join("attachments").exists());
}

// verifies: PROTO-CORE-API-R23
#[cfg(unix)]
#[tokio::test]
async fn a_linked_ersa_folder_is_never_written_through() {
    let mut s = setup(FakeAdapter::new(1)).await;
    let id = s.session().await;
    let notes = path(&file(&s.outside, "notes.md", b"x"));
    let elsewhere = s.core.dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, s.workspace.join(".ersa")).unwrap();
    let response = s
        .send(&id, "", json!([{ "kind": "file", "path": notes }]))
        .await;
    refused(&response, 0);
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}

// verifies: PROTO-CORE-API-R25
#[tokio::test]
async fn interrupt_cancels_a_send_still_waiting_for_its_backend() {
    let mut s = setup(FakeAdapter::slow_start(Duration::from_secs(5))).await;
    let id = s.session().await;
    let notes = path(&file(&s.outside, "notes.md", b"x"));
    let send = s
        .client
        .request(
            "session.send",
            json!({ "id": id, "text": "hi", "attachments": [{ "kind": "file", "path": notes }] }),
        )
        .await;
    // Copied, and now waiting for the backend to start.
    s.client
        .notification_where(|n| {
            n["method"] == "session.send_progress" && n["params"]["phase"] == "starting"
        })
        .await;
    assert_eq!(copies(&s.scratch(&id)), vec!["1"]);
    // On the same connection: the send does not hold the interrupt up.
    let interrupted = s
        .client
        .call("session.interrupt", json!({ "id": id }))
        .await;
    assert!(interrupted.get("result").is_some(), "{interrupted:#}");
    let response = s.client.response(send).await;
    assert_eq!(kind(&response["error"]), "invalid_state", "{response:#}");
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("stopped")
    );
    // Nothing sent, no copy left, and the session can send again.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(copies(&s.scratch(&id)), Vec::<String>::new());
    assert!(s.messages().is_empty());
    let again = s.send(&id, "again", json!([])).await;
    assert!(again.get("result").is_some(), "{again:#}");
    assert_eq!(s.messages().len(), 1);
}

// verifies: PROTO-CORE-API-R26
#[tokio::test]
async fn a_send_reports_its_phases_and_answers_once_the_backend_has_it() {
    let mut s = setup(FakeAdapter::slow_start(Duration::from_secs(2))).await;
    let id = s.session().await;
    let notes = path(&file(&s.outside, "notes.md", b"x"));
    let send = s
        .client
        .request(
            "session.send",
            json!({ "id": id, "text": "hi", "attachments": [{ "kind": "file", "path": notes }] }),
        )
        .await;
    let first = s
        .client
        .notification_where(|n| n["method"] == "session.send_progress")
        .await;
    assert_eq!(first["params"]["session_id"], id.as_str());
    s.client
        .notification_where(|n| {
            n["method"] == "session.send_progress" && n["params"]["phase"] == "starting"
        })
        .await;
    // `session.list` says so too, and a second send meanwhile is refused.
    let mut other = s.core.client().await;
    let sessions = other.result("session.list", json!({})).await;
    assert_eq!(sessions[0]["sending"], "starting");
    let second = other
        .error("session.send", json!({ "id": id, "text": "too" }))
        .await;
    assert_eq!(kind(&second), "invalid_state");

    let response = s.client.response(send).await;
    assert!(
        response["result"]["turn"].as_u64().unwrap() >= 1,
        "{response:#}"
    );
    assert_eq!(s.messages().len(), 1);
    let sessions = other.result("session.list", json!({})).await;
    assert!(sessions[0].get("sending").is_none(), "{sessions:#}");
}

// verifies: PROTO-CORE-API-R26
#[tokio::test]
async fn a_handover_that_breaks_is_delivery_unknown_and_keeps_its_copies() {
    let mut s = setup(FakeAdapter::new(1)).await;
    let id = s.session().await;
    let notes = path(&file(&s.outside, "notes.md", b"x"));
    let response = s
        .send(
            &id,
            "broken pipe",
            json!([{ "kind": "file", "path": notes }]),
        )
        .await;
    assert_eq!(kind(&response["error"]), "delivery_unknown", "{response:#}");
    // Removing an unsent message's copies happens just after its answer:
    // wait long enough that a removal would have shown.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(copies(&s.scratch(&id)), vec!["1"]);
    // The session can send again.
    let again = s.send(&id, "again", json!([])).await;
    assert!(again.get("result").is_some(), "{again:#}");
}

// verifies: PROTO-CORE-API-R26
#[tokio::test]
async fn a_session_found_stopped_after_its_line_was_written_is_delivery_unknown() {
    let mut s = setup(FakeAdapter::new(1)).await;
    let id = s.session().await;
    let notes = path(&file(&s.outside, "notes.md", b"x"));
    let response = s
        .send(
            &id,
            "stopped mid-write",
            json!([{ "kind": "file", "path": notes }]),
        )
        .await;
    assert_eq!(kind(&response["error"]), "delivery_unknown", "{response:#}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(copies(&s.scratch(&id)), vec!["1"]);
}

// verifies: PROTO-CORE-API-R28
#[tokio::test]
async fn removing_a_session_removes_its_scratch_folder_only() {
    let mut s = setup(FakeAdapter::new(1)).await;
    let first = s.session().await;
    let second = s.session().await;
    let notes = path(&file(&s.outside, "notes.md", b"x"));
    for id in [&first, &second] {
        let response = s
            .send(id, "", json!([{ "kind": "file", "path": notes }]))
            .await;
        assert!(response.get("result").is_some(), "{response:#}");
    }
    s.client
        .result("session.stop", json!({ "id": first }))
        .await;
    s.client
        .result("session.remove", json!({ "id": first }))
        .await;
    assert!(!s.scratch(&first).exists());
    assert_eq!(copies(&s.scratch(&second)), vec!["1"]);
    // The file it was copied from is untouched.
    assert!(Path::new(&notes).exists());
}

// verifies: PROTO-CORE-API-R24
#[tokio::test]
async fn inspect_reports_kind_size_and_lines() {
    let mut s = setup(FakeAdapter::new(1)).await;
    let text = file(&s.outside, "a.txt", b"one\ntwo\nthree");
    let png = file(&s.outside, "shot.dat", PNG);
    let binary = file(&s.outside, "b.bin", b"\0\x01\x02");

    let inspected = s
        .client
        .result(
            "attachment.inspect",
            json!({ "host_id": "local", "path": path(&text) }),
        )
        .await;
    assert_eq!(inspected["kind"], "file");
    assert_eq!(inspected["name"], "a.txt");
    assert_eq!(inspected["size_bytes"], 13);
    assert_eq!(inspected["lines"], 3);

    // Told by its first bytes, not its name.
    let inspected = s
        .client
        .result(
            "attachment.inspect",
            json!({ "host_id": "local", "path": path(&png) }),
        )
        .await;
    assert_eq!(inspected["kind"], "image");
    assert_eq!(inspected["media_type"], "image/png");
    assert_eq!(inspected["size_bytes"], PNG.len());
    assert!(inspected.get("lines").is_none());

    let inspected = s
        .client
        .result(
            "attachment.inspect",
            json!({ "host_id": "local", "path": path(&binary) }),
        )
        .await;
    assert_eq!(inspected["kind"], "file");
    assert!(inspected.get("lines").is_none());

    let inspected = s
        .client
        .result(
            "attachment.inspect",
            json!({ "host_id": "local", "path": path(&s.outside) }),
        )
        .await;
    assert_eq!(inspected["kind"], "folder");
    assert!(inspected.get("size_bytes").is_none());

    let missing = s
        .client
        .error(
            "attachment.inspect",
            json!({ "host_id": "local", "path": path(&s.outside.join("gone")) }),
        )
        .await;
    assert_eq!(kind(&missing), "not_found");
    let relative = s
        .client
        .error(
            "attachment.inspect",
            json!({ "host_id": "local", "path": "a.txt" }),
        )
        .await;
    assert_eq!(relative["code"], -32602);
}

// verifies: PROTO-CORE-API-R24
#[cfg(unix)]
#[tokio::test]
async fn inspect_refuses_a_pipe_and_reports_a_links_target() {
    let mut s = setup(FakeAdapter::new(1)).await;
    let pipe = s.outside.join("pipe");
    let made = std::process::Command::new("mkfifo")
        .arg(&pipe)
        .status()
        .unwrap();
    assert!(made.success());
    let refused = s
        .client
        .error(
            "attachment.inspect",
            json!({ "host_id": "local", "path": path(&pipe) }),
        )
        .await;
    assert_eq!(refused["code"], -32602);

    let target = file(&s.outside, "a.txt", b"x\n");
    let link = s.outside.join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let inspected = s
        .client
        .result(
            "attachment.inspect",
            json!({ "host_id": "local", "path": path(&link) }),
        )
        .await;
    assert_eq!(inspected["name"], "link.txt");
    assert_eq!(inspected["target"], path(&target));
    assert_eq!(inspected["lines"], 1);
}
