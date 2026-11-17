//! A schedule a test chooses: which of Core's operations begins next, and
//! which of the calls they wait on (the fake adapter's, the command
//! transport's) completes next.
//!
//! Operations run on one thread with tokio's clock paused. Each call the
//! test orders waits at a [`Schedule::point`]. [`drive`] lets everything
//! run until it is parked, then takes one step: it begins an operation, or
//! lets one waiting call go. [`every_schedule`] runs the test once for
//! every way those steps can be chosen, so every order of arrivals and
//! completions runs, and a failure names the choices that led to it.
//!
//! "Parked" is exact, not a pause: under a paused clock a sleep returns
//! only once no task can run, and tokio holds the clock while a blocking
//! call (the store's, a command's) is in flight.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;

/// The variable that runs one schedule alone: the choices a failure printed.
pub const SCHEDULE: &str = "ERSA_SCHEDULE";

/// Whether one schedule is being run alone ([`SCHEDULE`]): a test then
/// checks that schedule only, not how many there are or what they reach.
pub fn replaying() -> bool {
    std::env::var(SCHEDULE).is_ok()
}

/// How much of the paused clock one run may use while its operations wait
/// on nothing the test orders: past it, an operation is stuck.
const PATIENCE: Duration = Duration::from_secs(600);

/// The calls waiting for the test to let them go.
#[derive(Default)]
pub struct Schedule {
    inner: Mutex<Waiting>,
}

#[derive(Default)]
struct Waiting {
    /// Whether calls wait at their points; off, they pass straight through.
    on: bool,
    calls: Vec<(String, oneshot::Sender<()>)>,
}

impl Schedule {
    /// A call named `label` waits here until the test lets it go. Passes
    /// straight through while the schedule is off, as during set-up.
    pub async fn point(&self, label: &str) {
        let let_go = {
            let mut waiting = self.inner.lock().unwrap();
            if !waiting.on {
                return;
            }
            let (go, let_go) = oneshot::channel();
            waiting.calls.push((label.to_string(), go));
            let_go
        };
        // The schedule turned off lets it go too.
        let _ = let_go.await;
    }

    /// From here calls wait at their points.
    pub fn turn_on(&self) {
        self.inner.lock().unwrap().on = true;
    }

    /// Lets every waiting call go, and every later one pass.
    pub fn turn_off(&self) {
        let mut waiting = self.inner.lock().unwrap();
        waiting.on = false;
        waiting.calls.clear();
    }

    /// The labels of the calls waiting, each once, in order: the same
    /// whatever order they arrived in.
    fn waiting(&self) -> Vec<String> {
        let waiting = self.inner.lock().unwrap();
        let mut labels: Vec<String> = waiting.calls.iter().map(|(l, _)| l.clone()).collect();
        labels.sort();
        labels.dedup();
        labels
    }

    /// Lets go the call labelled `label` that has waited longest.
    fn let_go(&self, label: &str) {
        let mut waiting = self.inner.lock().unwrap();
        let place = waiting.calls.iter().position(|(l, _)| l == label);
        let (_, go) = waiting.calls.remove(place.expect("a waiting call"));
        let _ = go.send(());
    }
}

/// One run's choices: at each decision, which option is taken. Shared, so
/// that whoever ran the schedule can say what it was when the run panics.
#[derive(Clone, Default)]
pub struct Choices(Arc<Mutex<Made>>);

#[derive(Default)]
struct Made {
    /// The choices to make first; past them, the first option.
    given: Vec<usize>,
    /// What each of the first choices was when it was made before, when
    /// that is known: the same choices must meet the same steps again.
    expected: Vec<String>,
    /// Each decision made: what was taken, of how many, and what it was.
    taken: Vec<(usize, usize, String)>,
}

impl Choices {
    /// The choices `seed` names, as a failure printed them: numbers joined
    /// by dots.
    pub fn replay(seed: &str) -> Choices {
        let picks = seed.split('.').filter(|pick| !pick.is_empty());
        let given = picks
            .map(|pick| pick.parse().expect("a schedule is numbers joined by dots"))
            .collect();
        let (expected, taken) = (Vec::new(), Vec::new());
        let made = Made {
            given,
            expected,
            taken,
        };
        Choices(Arc::new(Mutex::new(made)))
    }

    /// These choices again, with one more: the last decision that had an
    /// option left takes its next one, and what followed it is dropped.
    /// `None` once every option of every decision was taken.
    fn next(&self) -> Option<Choices> {
        let made = self.0.lock().unwrap();
        let mut picks: Vec<usize> = made.taken.iter().map(|(p, _, _)| *p).collect();
        while let Some(last) = picks.pop() {
            let options = made.taken[picks.len()].1;
            if last + 1 < options {
                // Up to the changed decision, the steps are known.
                let steps = made.taken[..picks.len()].iter();
                let expected = steps.map(|(_, _, step)| step.clone()).collect();
                picks.push(last + 1);
                let (given, taken) = (picks, Vec::new());
                let made = Made {
                    given,
                    expected,
                    taken,
                };
                return Some(Choices(Arc::new(Mutex::new(made))));
            }
        }
        None
    }

    /// Which of `options` is taken at this decision.
    fn choose(&self, options: &[String]) -> usize {
        let mut made = self.0.lock().unwrap();
        let at = made.taken.len();
        let pick = made.given.get(at).copied().unwrap_or(0);
        assert!(
            pick < options.len(),
            "the schedule does not fit: decision {at} has only {options:?}"
        );
        // The schedule's tree is the same on every run: a choice made
        // again leads to the step it led to before. Were it not, a branch
        // could be skipped without a sign.
        if let Some(before) = made.expected.get(at) {
            assert_eq!(
                &options[pick], before,
                "decision {at} is not what it was: the schedule is not deterministic"
            );
        }
        made.taken
            .push((pick, options.len(), options[pick].clone()));
        pick
    }

    /// The steps taken, in order, in words.
    pub fn steps(&self) -> String {
        let made = self.0.lock().unwrap();
        let steps: Vec<&str> = made.taken.iter().map(|(_, _, s)| s.as_str()).collect();
        steps.join(", ")
    }

    /// The choices made, as [`SCHEDULE`] takes them.
    pub fn seed(&self) -> String {
        let made = self.0.lock().unwrap();
        let picks: Vec<String> = made.taken.iter().map(|(p, _, _)| p.to_string()).collect();
        picks.join(".")
    }
}

/// Runs `run` once for every schedule: every way its decisions can be
/// taken, depth first. Returns how many there were. A run that panics
/// prints its choices, which [`SCHEDULE`] set to them runs alone: name the
/// test too, since the choices fit only the scenario that printed them.
pub fn every_schedule(name: &str, mut run: impl FnMut(Choices)) -> usize {
    let only = std::env::var(SCHEDULE).ok();
    let mut next = Some(Choices::replay(only.as_deref().unwrap_or_default()));
    let mut count = 0;
    while let Some(choices) = next {
        if let Err(panic) = catch_unwind(AssertUnwindSafe(|| run(choices.clone()))) {
            let (seed, steps) = (choices.seed(), choices.steps());
            eprintln!("{name} failed under {SCHEDULE}={seed}: {steps}");
            resume_unwind(panic);
        }
        count += 1;
        next = match only {
            Some(_) => None,
            None => choices.next(),
        };
    }
    count
}

/// An operation of Core's for [`drive`] to begin: its name and what it does.
pub type Operation<T> = (&'static str, Pin<Box<dyn Future<Output = T> + Send>>);

/// Runs `operations` to their ends under `choices`, and returns what each
/// answered, in the order given. Each decision is taken once everything
/// is parked: an operation not begun yet begins, or one call waiting at a
/// point of `schedule` is let go. Must run on a current-thread runtime
/// with the clock paused.
pub async fn drive<T: Send + 'static>(
    schedule: &Schedule,
    choices: &Choices,
    operations: Vec<Operation<T>>,
) -> Vec<T> {
    let mut answers: Vec<Option<T>> = operations.iter().map(|_| None).collect();
    let mut to_begin: Vec<(usize, Operation<T>)> = operations.into_iter().enumerate().collect();
    let mut under_way: Vec<(usize, &'static str, tokio::task::JoinHandle<T>)> = Vec::new();
    let began = tokio::time::Instant::now();
    loop {
        // Until everything is parked.
        tokio::time::sleep(Duration::from_millis(1)).await;
        let mut still = Vec::new();
        for (place, name, task) in under_way.drain(..) {
            if task.is_finished() {
                answers[place] = Some(task.await.expect("the operation did not panic"));
            } else {
                still.push((place, name, task));
            }
        }
        under_way = still;
        if to_begin.is_empty() && under_way.is_empty() {
            break;
        }
        let waiting = schedule.waiting();
        let mut options: Vec<String> = Vec::new();
        options.extend(
            to_begin
                .iter()
                .map(|(_, (name, _))| format!("begin {name}")),
        );
        options.extend(waiting.iter().map(|label| format!("let go {label}")));
        if options.is_empty() {
            // The operations wait on a timer of Core's own: let it run out.
            let names: Vec<&str> = under_way.iter().map(|(_, name, _)| *name).collect();
            assert!(
                began.elapsed() < PATIENCE,
                "stuck: {names:?} wait on nothing the schedule holds"
            );
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }
        let pick = choices.choose(&options);
        if pick < to_begin.len() {
            let (place, (name, operation)) = to_begin.remove(pick);
            under_way.push((place, name, tokio::spawn(operation)));
        } else {
            schedule.let_go(&waiting[pick - to_begin.len()]);
        }
    }
    answers
        .into_iter()
        .map(|answer| answer.expect("every operation answered"))
        .collect()
}
