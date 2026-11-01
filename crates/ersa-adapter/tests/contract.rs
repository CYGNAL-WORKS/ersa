//! A minimal adapter written against the traits, end to end over the fake
//! transport. It shows the contract in use (and proves every trait works as a
//! trait object): an adapter hands every line in both directions to the sink,
//! records host lines before writing them, reports the exit once, and its
//! projector turns the resulting raw log into events that pass the schema.
//!
//! It is deliberately not a real backend: no Claude Code types appear here.

use std::io;
use std::sync::{Arc, Mutex};

use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, Direction, LineWriter, PermissionDecision,
    Process, ProcessExit, ProcessSpec, Projected, Projector, RawLine, SessionConfig, Transport,
    async_trait,
};
use ersa_protocol::events::{BackendPassthrough, Event, PermissionBehavior};
use ersa_protocol::schema::validate_event;

// ---------------------------------------------------------------------------
// The adapter

struct EchoAdapter;

struct EchoSession {
    sink: Arc<dyn AdapterSink>,
    stdin: tokio::sync::Mutex<Box<dyn LineWriter>>,
    process: Arc<tokio::sync::Mutex<Box<dyn Process>>>,
}

#[async_trait]
impl Adapter for EchoAdapter {
    fn backend(&self) -> &str {
        "echo"
    }

    async fn start(
        &self,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError> {
        let mut process = transport
            .spawn(ProcessSpec {
                program: "echo-backend".into(),
                args: vec!["--permission-mode".into(), config.permission_mode],
                cwd: Some(config.cwd),
                ..ProcessSpec::default()
            })
            .await?;
        let stdin = process.take_stdin().ok_or(AdapterError::NotRunning)?;
        let mut stdout = process.take_stdout().ok_or(AdapterError::NotRunning)?;
        let process = Arc::new(tokio::sync::Mutex::new(process));

        // The handshake: `start` returns once the backend has announced itself,
        // so the announcement is in the raw log before any user message.
        let hello = stdout.next_line().await?.ok_or(AdapterError::NotRunning)?;
        sink.record(Direction::Backend, hello);

        // The read loop: every stdout line to the sink, then the exit, once.
        let (loop_sink, loop_process) = (sink.clone(), process.clone());
        tokio::spawn(async move {
            while let Ok(Some(line)) = stdout.next_line().await {
                loop_sink.record(Direction::Backend, line);
            }
            let exit = loop_process.lock().await.wait().await;
            loop_sink.process_exited(exit);
        });

        Ok(Box::new(EchoSession {
            sink,
            stdin: tokio::sync::Mutex::new(stdin),
            process,
        }))
    }

    fn projector(&self) -> Box<dyn Projector> {
        Box::new(EchoProjector)
    }
}

impl EchoSession {
    /// Records the line, then writes it: the order `AdapterSink::record` asks for.
    async fn write(&self, line: String) -> Result<(), AdapterError> {
        let mut stdin = self.stdin.lock().await;
        self.sink.record(Direction::Host, line.clone());
        stdin.write_line(&line).await?;
        Ok(())
    }
}

#[async_trait]
impl AdapterSession for EchoSession {
    async fn send_user(&self, text: &str) -> Result<(), AdapterError> {
        self.write(serde_json::json!({ "user": text }).to_string())
            .await
    }

    async fn decide_permission(
        &self,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), AdapterError> {
        let allow = decision.behavior == PermissionBehavior::Allow;
        self.write(serde_json::json!({ "answer": request_id, "allow": allow }).to_string())
            .await
    }

    async fn interrupt(&self) -> Result<(), AdapterError> {
        self.write(r#"{"interrupt":true}"#.into()).await
    }

    async fn stop(&self) -> Result<(), AdapterError> {
        self.process.lock().await.kill().await?;
        Ok(())
    }
}

/// Projects every line to a passthrough: the fallback every real projector has.
struct EchoProjector;

impl Projector for EchoProjector {
    fn version(&self) -> u32 {
        1
    }

    fn project(&mut self, line: &RawLine) -> Vec<Projected> {
        let payload = serde_json::from_str(&line.line)
            .unwrap_or_else(|_| serde_json::Value::String(line.line.clone()));
        let kind = match line.dir {
            Direction::Backend => "backend",
            Direction::Host => "host",
            Direction::Core => "core",
        };
        vec![Projected::from_line(
            line,
            Event::BackendPassthrough(BackendPassthrough {
                backend: "echo".into(),
                kind: kind.into(),
                payload,
            }),
        )]
    }
}

// ---------------------------------------------------------------------------
// Core's side: a sink that stamps lines the way Core's raw log will.

#[derive(Default)]
struct RecordingSink {
    lines: Mutex<Vec<RawLine>>,
    exits: Mutex<Vec<io::Result<ProcessExit>>>,
    exited: tokio::sync::Notify,
}

impl AdapterSink for RecordingSink {
    fn record(&self, dir: Direction, line: String) {
        let mut lines = self.lines.lock().unwrap();
        let index = lines.len() as u64;
        lines.push(RawLine {
            index,
            // A fixed time keeps the test deterministic; Core uses the clock.
            ts: "2026-10-31T12:00:00Z".into(),
            dir,
            line,
        });
    }

    fn stderr_line(&self, _line: String) {}

    fn process_exited(&self, exit: io::Result<ProcessExit>) {
        self.exits.lock().unwrap().push(exit);
        self.exited.notify_one();
    }
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_adapter_session_round_trips_through_the_traits() {
    let transport = FakeTransport::new([r#"{"hello":"from backend"}"#]).hold_open();
    let sink = Arc::new(RecordingSink::default());
    // Everything below sees only trait objects, as Core will.
    let adapter: Box<dyn Adapter> = Box::new(EchoAdapter);

    let config = SessionConfig {
        cwd: "/work".into(),
        permission_mode: "default".into(),
        ..SessionConfig::default()
    };
    let session = adapter
        .start(Arc::new(transport.clone()), config, sink.clone())
        .await
        .expect("start");

    session.send_user("hi").await.expect("send");
    session
        .decide_permission(
            "r-1",
            PermissionDecision {
                behavior: PermissionBehavior::Allow,
                message: None,
                updated_input: None,
                apply_suggestions: vec![],
            },
        )
        .await
        .expect("decide");
    session.stop().await.expect("stop");
    sink.exited.notified().await;

    // The process was started as configured and got exactly the host lines.
    let spawned = transport.spawned();
    assert_eq!(spawned[0].args, ["--permission-mode", "default"]);
    assert_eq!(spawned[0].cwd.as_deref(), Some("/work"));
    assert_eq!(
        transport.stdin_lines(),
        [r#"{"user":"hi"}"#, r#"{"allow":true,"answer":"r-1"}"#]
    );

    // The raw log holds both directions, gap-free, and one exit was reported.
    let lines = sink.lines.lock().unwrap().clone();
    let dirs: Vec<_> = lines.iter().map(|l| (l.index, l.dir)).collect();
    assert_eq!(
        dirs,
        [
            (0, Direction::Backend),
            (1, Direction::Host),
            (2, Direction::Host)
        ]
    );
    assert_eq!(sink.exits.lock().unwrap().len(), 1);

    // Projection: every line yields an event, the events pass the schema, and
    // projecting twice gives the same result.
    let project = |lines: &[RawLine]| {
        let mut projector = adapter.projector();
        lines
            .iter()
            .flat_map(|l| projector.project(l))
            .collect::<Vec<_>>()
    };
    let events = project(&lines);
    assert_eq!(events.len(), lines.len());
    for projected in &events {
        let payload = serde_json::to_value(&projected.event).expect("serialize");
        validate_event(&payload).unwrap_or_else(|e| panic!("{e}\n{payload}"));
    }
    assert_eq!(project(&lines), events);
}
