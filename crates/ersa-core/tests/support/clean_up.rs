//! A Core for the Clean up list (FLOW-CLEAN-UP, The list): the fake backend,
//! a git project, and a sweep whose clock is the test's (`Sweeps::Driven`),
//! so a test moves time on without a pass. The list reads the same clock.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ersa_core::{Adapter, CoreConfig, SweepClock, Sweeps};
use serde_json::{Value, json};

use super::fake::FakeAdapter;
use super::put_away::{Setup, id};
use super::{assert_valid, start_with_adapters, tempdir};

pub const DAY: Duration = Duration::from_secs(24 * 60 * 60);
pub const HOUR: Duration = Duration::from_secs(60 * 60);

/// A Core with the fake backend and a git project, whose sweep runs only
/// at start and after an archive, by the returned clock.
pub async fn listing_with(more: impl FnOnce(&mut CoreConfig)) -> (Setup, Arc<SweepClock>) {
    listing_over(FakeAdapter::new(1), more).await
}

pub async fn listing() -> (Setup, Arc<SweepClock>) {
    listing_with(|_| {}).await
}

/// [`listing_with`], over `fake`.
pub async fn listing_over(
    fake: FakeAdapter,
    more: impl FnOnce(&mut CoreConfig),
) -> (Setup, Arc<SweepClock>) {
    let clock = Arc::new(SweepClock::default());
    let stats = Arc::clone(&fake.stats);
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(fake)];
    let sweeps = Sweeps::Driven(Arc::clone(&clock));
    let configure = move |config: &mut CoreConfig| {
        config.sweeps = sweeps;
        more(config);
    };
    let core = start_with_adapters(tempdir(), configure, adapters).await;
    let s = Setup::over(core, stats).await;
    passes(&clock, 1).await;
    (s, clock)
}

pub async fn passes(clock: &SweepClock, count: u64) {
    tokio::time::timeout(Duration::from_secs(120), clock.until_ended(count))
        .await
        .expect("the sweep ended in time");
}

/// `cleanup.list` with `inactive_days`, checked against the schema.
pub async fn list(s: &mut Setup, days: u32) -> Value {
    let response = s
        .client
        .call("cleanup.list", json!({ "inactive_days": days }))
        .await;
    assert_valid("cleanup.list", &response);
    response["result"].clone()
}

/// `cleanup.inspect` of `id`, checked against the schema.
pub async fn inspect(s: &mut Setup, id: &str) -> Value {
    let response = s.client.call("cleanup.inspect", json!({ "id": id })).await;
    assert_valid("cleanup.inspect", &response);
    assert!(response.get("result").is_some(), "{response}");
    response["result"].clone()
}

/// The item id of `workspace`.
pub fn ws_id(workspace: &Value) -> String {
    format!("w:{}", id(workspace))
}

/// A session of the fake backend in `workspace`, with a turn behind it,
/// then stopped and not subscribed to, so it can count as unused.
pub async fn session_in(s: &mut Setup, workspace: &Value) -> String {
    let session = s.stopped_session(workspace).await;
    s.client
        .result("session.unsubscribe", json!({ "id": session }))
        .await;
    session
}

/// Archives `session`, then waits for the pass the archive asks for.
pub async fn archive(s: &mut Setup, clock: &SweepClock, session: &str) {
    let before = clock.ended();
    let params = json!({ "id": session, "archived": true });
    s.client.result("session.set_archived", params).await;
    passes(clock, before + 1).await;
}

/// The project's folder under the worktrees root.
pub fn worktrees(s: &Setup) -> PathBuf {
    let root = s.core.dir.path().join("worktrees");
    let name = s.project["name"].as_str().unwrap();
    root.join(name)
}
