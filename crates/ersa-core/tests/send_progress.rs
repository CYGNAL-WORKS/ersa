//! When a send's progress goes out (PROTO-CORE-API, Notifications:
//! `session.send_progress`), driven through [`Pacer`] on tokio's paused
//! clock, so the times are exact. The delivery of those reports over the
//! socket is tested in `attachments.rs`.

use ersa_core::sessions::Pacer;
use ersa_protocol::core_api::{SendPhase, SendProgressParams};
use tokio::sync::Notify;
use tokio::time::{Duration, Instant};

fn progress(phase: SendPhase, copied: Option<u64>) -> SendProgressParams {
    SendProgressParams {
        session_id: "s-1".into(),
        phase,
        copied_bytes: copied,
        total_bytes: copied.map(|_| 100),
    }
}

/// Feeds the pacer `current()` each time it wakes until `until`, and
/// returns when each report went out, from the start.
async fn sent_until(
    pacer: &mut Pacer,
    changed: &Notify,
    until: Duration,
    mut current: impl FnMut(Duration) -> SendProgressParams,
) -> Vec<Duration> {
    let began = Instant::now();
    let mut sent = Vec::new();
    while began.elapsed() < until {
        if pacer.due(&current(began.elapsed())) {
            sent.push(began.elapsed());
        }
        pacer.wait(changed).await;
    }
    sent
}

// verifies: PROTO-CORE-API-R26
#[tokio::test(start_paused = true)]
async fn unchanged_progress_repeats_every_5_seconds() {
    let (mut pacer, changed) = (Pacer::default(), Notify::new());
    let sent = sent_until(&mut pacer, &changed, Duration::from_secs(16), |_| {
        progress(SendPhase::Starting, None)
    })
    .await;
    let secs = |s| Duration::from_secs(s);
    assert_eq!(sent, vec![secs(0), secs(5), secs(10), secs(15)]);
}

// verifies: PROTO-CORE-API-R26
#[tokio::test(start_paused = true)]
async fn a_new_phase_goes_out_at_once_but_never_twice_within_a_second() {
    let (mut pacer, changed) = (Pacer::default(), Notify::new());
    let began = Instant::now();
    assert!(pacer.due(&progress(SendPhase::Checking, None)));
    // The phase changes 200 ms later: it goes out a second after the last
    // report, not before.
    tokio::time::sleep(Duration::from_millis(200)).await;
    changed.notify_one();
    pacer.wait(&changed).await;
    assert!(pacer.due(&progress(SendPhase::Starting, None)));
    assert_eq!(began.elapsed(), Duration::from_secs(1));
}

// verifies: PROTO-CORE-API-R26
#[tokio::test(start_paused = true)]
async fn bytes_copied_go_out_once_a_second() {
    let (mut pacer, changed) = (Pacer::default(), Notify::new());
    let sent = sent_until(&mut pacer, &changed, Duration::from_secs(3), |elapsed| {
        // A copy that moves every 100 ms.
        progress(SendPhase::Copying, Some(elapsed.as_millis() as u64 / 100))
    })
    .await;
    let secs = |s| Duration::from_secs(s);
    assert_eq!(sent, vec![secs(0), secs(1), secs(2)]);
}
