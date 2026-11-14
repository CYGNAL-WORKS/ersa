//! How long the bridge waits for a `session.send` (PROTO-CORE-API-R89):
//! while `session.send_progress` for the send's session keeps arriving, and
//! 30 seconds of silence after the last one. Against a fake Core on an
//! in-memory pipe, on tokio's paused clock, so minutes pass at once.

use std::time::Duration;

use ersa_shell::bridge::{BridgeError, CoreLink, LinkOptions};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

/// Opens a link to a fake Core that answers `core.hello` and nothing
/// else, and returns the Core's end.
async fn open_fake() -> (CoreLink, BufReader<DuplexStream>) {
    let (shell_end, core_end) = tokio::io::duplex(1024);
    let open = CoreLink::open(Box::new(shell_end), LinkOptions::default(), |_| {});
    let mut core = BufReader::new(core_end);
    let answer = async {
        let mut hello = String::new();
        core.read_line(&mut hello).await.expect("hello arrives");
        reply_to(&mut core, &hello, json!({ "api_version": 5 })).await;
    };
    let (link, ()) = tokio::join!(open, answer);
    (link.expect("handshake"), core)
}

async fn reply_to(core: &mut BufReader<DuplexStream>, request: &str, result: Value) {
    let id = serde_json::from_str::<Value>(request).expect("json")["id"].clone();
    let reply = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    core.get_mut()
        .write_all(format!("{reply}\n").as_bytes())
        .await
        .expect("reply");
}

/// Core's report that a send to `session` is still under way.
async fn progress(core: &mut BufReader<DuplexStream>, session: &str) {
    let line = json!({
        "jsonrpc": "2.0",
        "method": "session.send_progress",
        "params": { "session_id": session, "phase": "copying" },
    });
    core.get_mut()
        .write_all(format!("{line}\n").as_bytes())
        .await
        .expect("progress");
}

fn send_to(session: &str) -> Option<Value> {
    Some(json!({ "id": session, "text": "hello" }))
}

const EVERY: Duration = Duration::from_secs(5);

/// A send Core reports on for longer than any fixed limit still gets
/// its answer. On a paused clock, so fifteen minutes pass at once.
// verifies: PROTO-CORE-API-R89
#[tokio::test(start_paused = true)]
async fn a_send_waits_while_its_progress_keeps_arriving() {
    let (link, mut core) = open_fake().await;
    let core_side = async {
        let mut send = String::new();
        core.read_line(&mut send).await.expect("the send");
        for _ in 0..(15 * 60 / 5) {
            tokio::time::sleep(EVERY).await;
            progress(&mut core, "s1").await;
        }
        reply_to(&mut core, &send, json!({ "turn": 1 })).await;
    };
    let (answer, ()) = tokio::join!(link.request("session.send", send_to("s1")), core_side);
    assert_eq!(answer, Ok(json!({ "turn": 1 })));
}

/// A send Core says nothing more about counts as unanswered after 30
/// seconds of silence, not after a longer fixed limit.
// verifies: PROTO-CORE-API-R89
#[tokio::test(start_paused = true)]
async fn a_silent_send_is_unanswered_after_thirty_seconds() {
    let (link, _core) = open_fake().await;
    let began = tokio::time::Instant::now();
    let answer = link.request("session.send", send_to("s1")).await;
    assert_eq!(
        answer,
        Err(BridgeError::unanswered("Ersa got no answer in time."))
    );
    let waited = began.elapsed();
    assert!(
        waited >= Duration::from_secs(30) && waited < Duration::from_secs(31),
        "{waited:?}"
    );
    assert!(!link.is_closed(), "one silent send must not drop the link");
}

/// The silence counts from the last report: a send that heard progress
/// for a while, then nothing, is unanswered 30 seconds after the last one.
// verifies: PROTO-CORE-API-R89
#[tokio::test(start_paused = true)]
async fn a_send_is_unanswered_thirty_seconds_after_its_last_progress() {
    let (link, mut core) = open_fake().await;
    let began = tokio::time::Instant::now();
    let core_side = async {
        let mut send = String::new();
        core.read_line(&mut send).await.expect("the send");
        for _ in 0..20 {
            tokio::time::sleep(EVERY).await;
            progress(&mut core, "s1").await;
        }
        core
    };
    let (answer, _core) = tokio::join!(link.request("session.send", send_to("s1")), core_side);
    assert_eq!(
        answer,
        Err(BridgeError::unanswered("Ersa got no answer in time."))
    );
    // The last report came at 100 seconds.
    let waited = began.elapsed();
    assert!(
        waited >= Duration::from_secs(130) && waited < Duration::from_secs(131),
        "{waited:?}"
    );
}

/// A second send queued behind a first on the same session waits while
/// the session's progress arrives, though it is all for the first.
// verifies: PROTO-CORE-API-R89
#[tokio::test(start_paused = true)]
async fn a_queued_send_waits_while_its_session_reports_progress() {
    let (link, mut core) = open_fake().await;
    let core_side = async {
        let (mut first, mut second) = (String::new(), String::new());
        core.read_line(&mut first).await.expect("the first send");
        core.read_line(&mut second).await.expect("the second send");
        for tick in 1..=(24 * 60 / 5) {
            tokio::time::sleep(EVERY).await;
            progress(&mut core, "s1").await;
            if tick == 12 * 60 / 5 {
                reply_to(&mut core, &first, json!({ "turn": 1 })).await;
            }
        }
        reply_to(&mut core, &second, json!({ "turn": 2 })).await;
    };
    let (first, second, ()) = tokio::join!(
        link.request("session.send", send_to("s1")),
        link.request("session.send", send_to("s1")),
        core_side
    );
    assert_eq!(first, Ok(json!({ "turn": 1 })));
    assert_eq!(second, Ok(json!({ "turn": 2 })));
}

/// Progress for another session does not keep a send waiting.
// verifies: PROTO-CORE-API-R89
#[tokio::test(start_paused = true)]
async fn progress_for_another_session_does_not_keep_a_send_waiting() {
    let (link, mut core) = open_fake().await;
    let began = tokio::time::Instant::now();
    let core_side = async {
        let mut send = String::new();
        core.read_line(&mut send).await.expect("the send");
        for _ in 0..24 {
            tokio::time::sleep(EVERY).await;
            progress(&mut core, "s2").await;
        }
    };
    let ((answer, waited), ()) = tokio::join!(
        async {
            let answer = link.request("session.send", send_to("s1")).await;
            (answer, began.elapsed())
        },
        core_side
    );
    assert_eq!(
        answer,
        Err(BridgeError::unanswered("Ersa got no answer in time."))
    );
    assert!(
        waited >= Duration::from_secs(30) && waited < Duration::from_secs(31),
        "{waited:?}"
    );
}
