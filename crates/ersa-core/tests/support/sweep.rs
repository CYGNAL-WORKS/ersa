//! A Core whose sweep runs by the test's clock (`Sweeps::Driven`): the test
//! says when an hour or more has passed, and waits for each pass to end
//! (FLOW-CLEAN-UP, The sweep).

use std::sync::Arc;
use std::time::Duration;

use ersa_core::{Adapter, CoreConfig, SweepClock, Sweeps};

use super::fake::FakeAdapter;
use super::put_away::Setup;
use super::{start_with_adapters, tempdir};

/// How long a test waits for a pass to end before it fails.
pub const PASS: Duration = Duration::from_secs(120);

/// A Core with the fake backend `fake` and a git project, sweeping by the
/// returned clock, its pass at start ended.
pub async fn swept_with(
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

pub async fn swept() -> (Setup, Arc<SweepClock>) {
    swept_with(FakeAdapter::new(1), |_| {}).await
}

/// Waits until `count` passes have ended.
pub async fn passes(clock: &SweepClock, count: u64) {
    tokio::time::timeout(PASS, clock.until_ended(count))
        .await
        .expect("the sweep ended in time");
}

/// Moves the sweep's clock on by `by`, then waits for a pass.
pub async fn later(clock: &SweepClock, by: Duration) {
    let before = clock.ended();
    clock.time_passed(by);
    clock.sweep_again();
    passes(clock, before + 1).await;
}
