//! Sessions whose backend is a real process tree, started through the
//! transport Core hands the adapter: stopping a session, or shutting Core
//! down, leaves no process behind, grandchildren included.

mod support;

use std::io;
use std::sync::Arc;

use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, Direction, PermissionDecision, Projected,
    Projector, RawLine, SessionConfig, Transport, async_trait,
};
use ersa_protocol::events::{BackendPassthrough, Event};
use serde_json::{Value, json};
use support::procs::{eventually_gone, parent_of_a_grandchild, running};
use support::{Client, TestCore, start_with_adapters, tempdir, workspace};
use tokio::sync::watch;

/// An adapter whose "backend" is a process that starts a grandchild and
/// prints its id. The read loop records stdout and watches for the exit;
/// `stop` kills the process, as a backend that will not stop is killed.
struct TreeAdapter;

#[async_trait]
impl Adapter for TreeAdapter {
    fn backend(&self) -> &str {
        "tree"
    }

    async fn start(
        &self,
        transport: Arc<dyn Transport>,
        _config: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError> {
        let mut process = transport.spawn(parent_of_a_grandchild(true)).await?;
        let mut stdout = process.take_stdout().ok_or(AdapterError::NotRunning)?;
        // Ready once the grandchild's id is in the log.
        let first = stdout.next_line().await?.ok_or(AdapterError::NotRunning)?;
        sink.record(Direction::Backend, first);
        let kill = watch::Sender::new(false);
        let mut killed = kill.subscribe();
        tokio::spawn(async move {
            let exit = tokio::select! {
                exit = process.wait() => exit,
                // Wrapped so the borrow of the flag ends inside: a held
                // `watch::Ref` is not `Send`.
                _ = async { let _ = killed.wait_for(|k| *k).await; } => {
                    let _ = process.kill().await;
                    process.wait().await
                }
            };
            sink.process_exited(exit);
            // `process` drops here, and with it whatever is left of the tree.
        });
        Ok(Box::new(TreeSession { kill }))
    }

    fn projector(&self) -> Box<dyn Projector> {
        Box::new(Passthrough)
    }
}

struct TreeSession {
    kill: watch::Sender<bool>,
}

#[async_trait]
impl AdapterSession for TreeSession {
    async fn send_user(&self, _: &str) -> Result<(), AdapterError> {
        Err(AdapterError::Io(io::ErrorKind::Unsupported.into()))
    }

    async fn decide_permission(&self, id: &str, _: PermissionDecision) -> Result<(), AdapterError> {
        Err(AdapterError::UnknownRequest(id.into()))
    }

    async fn interrupt(&self) -> Result<(), AdapterError> {
        Ok(())
    }

    async fn stop(&self) -> Result<(), AdapterError> {
        self.kill.send_replace(true);
        Ok(())
    }
}

struct Passthrough;

impl Projector for Passthrough {
    fn version(&self) -> u32 {
        1
    }

    fn project(&mut self, line: &RawLine) -> Vec<Projected> {
        vec![Projected::from_line(
            line,
            Event::BackendPassthrough(BackendPassthrough {
                backend: "tree".into(),
                kind: "line".into(),
                payload: Value::String(line.line.clone()),
            }),
        )]
    }
}

/// A Core with the tree adapter and a started session; returns the
/// session's id and its grandchild's process id.
async fn tree_session() -> (TestCore, Client, String, u32) {
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(TreeAdapter)];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;
    let workspace_id = workspace(&mut client, core.dir.path()).await;
    let session = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": TreeAdapter.backend() }),
        )
        .await;
    let id = session["id"].as_str().unwrap().to_string();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let printed = client.events_until("backend.passthrough").await;
    let pid: u32 = printed.last().unwrap()["event"]["payload"]
        .as_str()
        .unwrap()
        .trim()
        .parse()
        .expect("the grandchild's id");
    assert!(running(pid), "the grandchild started");
    (core, client, id, pid)
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_session_leaves_no_grandchild_behind() {
    let (_core, mut client, id, grandchild) = tree_session().await;
    let stopped = client.result("session.stop", json!({ "id": id })).await;
    assert_eq!(stopped["state"], "ended");
    eventually_gone(grandchild).await;
}

// verifies: PROTO-CORE-API-R03
#[tokio::test(flavor = "multi_thread")]
async fn core_shutdown_stops_running_sessions_and_their_trees() {
    let (core, mut client, id, grandchild) = tree_session().await;
    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();
    eventually_gone(grandchild).await;

    // The next Core finds the session ended by the user, not cut off: the
    // app log's last fact about it is the user's stop.
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(TreeAdapter)];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    let sessions = client.result("session.list", json!({})).await;
    assert_eq!(sessions[0]["id"], id.as_str());
    assert_eq!(sessions[0]["state"], "ended");
    let facts = ersa_core::app_log::read_entries(&core.data_dir()).unwrap();
    let last = facts.iter().rfind(|e| e.session_id == id).unwrap();
    assert!(
        matches!(
            last.fact,
            ersa_core::app_log::LifecycleFact::ProcessExited {
                stopped_by_user: true,
                ..
            }
        ),
        "{last:?}"
    );
}
