//! A Core whose sweep runs by the test's clock (`Sweeps::Driven`): the test
//! says when an hour or more has passed, and waits for each pass to end
//! (FLOW-CLEAN-UP, The sweep).

use std::sync::Arc;
use std::time::Duration;

use ersa_core::{Adapter, CoreConfig, Gate, SweepClock, Sweeps};

use super::fake::FakeAdapter;
use super::put_away::Setup;
use super::{shut_down, start_with_adapters, tempdir};

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

/// Waits until the pass after `ended` passes stops at `gate`, at a try at a
/// folder: one that comes after the pass has judged a workspace and git has
/// failed to remove it, which is most of a pass, so waited for as long as a
/// pass is. A pass that ends without stopping there fails at once.
pub async fn reaches_the_try(gate: &Gate, clock: &SweepClock, ended: u64) {
    tokio::select! {
        () = gate.reached() => {}
        () = clock.until_ended(ended + 1) => panic!("the pass ended without trying the folder"),
        () = tokio::time::sleep(PASS) => panic!("the pass reached no try at the folder in {PASS:?}"),
    }
}

/// Moves the sweep's clock on by `by`, then waits for a pass.
pub async fn later(clock: &SweepClock, by: Duration) {
    let before = clock.ended();
    clock.time_passed(by);
    clock.sweep_again();
    passes(clock, before + 1).await;
}

/// Core restarted over `s`'s data, with `adapters` and the sweep on `clock`,
/// after `between` has run on the data folder.
pub async fn restarted_after(
    s: Setup,
    adapters: Vec<Arc<dyn Adapter>>,
    clock: &Arc<SweepClock>,
    between: impl FnOnce(&std::path::Path),
) -> Setup {
    let Setup {
        core,
        client,
        repo,
        project,
        stats,
    } = s;
    drop(client);
    let dir = shut_down(core).await;
    between(&dir.path().join("data"));
    let sweeps = Sweeps::Driven(Arc::clone(clock));
    let core = start_with_adapters(dir, move |config| config.sweeps = sweeps, adapters).await;
    let client = core.client().await;
    Setup {
        core,
        client,
        repo,
        project,
        stats,
    }
}

/// Core restarted over `s`'s data, with `adapters` and the sweep on `clock`.
pub async fn restarted(
    s: Setup,
    adapters: Vec<Arc<dyn Adapter>>,
    clock: &Arc<SweepClock>,
) -> Setup {
    restarted_after(s, adapters, clock, |_| {}).await
}
