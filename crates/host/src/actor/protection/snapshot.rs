//! One bounded snapshot of the sandbox (#813's reviews): the detection, the set-up's state and
//! a finished set-up's time, all read together on one thread of its own
//! (`clauseo-sandbox-detect`), so a read that hangs (macOS' first `sandbox-exec`, Windows' result
//! file, marker or admin copy) holds neither the host's start nor the quit that waits for it,
//! and the reads can't disagree.
//!
//! - At most one snapshot thread runs per seam ([`Slot`]): a request while one is still out
//!   (one that hung past an earlier wait) waits for that one instead of starting another, so a
//!   hang costs one thread however many retries come.
//! - A removal of the set-up (`RemoveSandboxSetup`) starts a new generation ([`Slot::removed`]):
//!   no request after it joins a thread started before it, whose answer may predate the
//!   removal (tier B, set up); that answer is discarded and a fresh snapshot decides.
//! - Each request waits at most its own `wait`; past it the answer is [`NoAnswer`], which the
//!   caller turns into a failed probe. A late answer still lands in the slot for whoever waits
//!   then.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use clauseo_protocol::Timestamp;
use clauseo_sandbox::{Detection, SetupState};

use crate::config::Sandbox;

/// A poisoned lock is still usable: each guarded value is whole between statements.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The thread's name.
const THREAD_NAME: &str = "clauseo-sandbox-detect";

/// What one snapshot read: the detection, the set-up's state where the seam has a set-up, and
/// when a finished set-up finished.
#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    pub(crate) detection: Detection,
    pub(crate) state: Option<SetupState>,
    pub(crate) set_up_at: Option<Timestamp>,
}

/// Why no snapshot came, by its log kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NoAnswer(pub(crate) &'static str);

/// A seam's snapshot thread, if one is out (shared by every clone of the seam).
#[derive(Default)]
pub(crate) struct Slot {
    flight: Mutex<Out>,
    /// The snapshot threads started (tests count them).
    #[cfg(test)]
    pub(crate) started: std::sync::atomic::AtomicUsize,
}

/// The slot's state: its generation (one per removal) and the thread out, with the generation
/// it started in.
#[derive(Default)]
struct Out {
    generation: u64,
    flight: Option<(u64, Arc<Flight>)>,
}

impl Slot {
    /// The set-up was removed: a snapshot thread started before now is never joined again, and
    /// its answer is discarded.
    pub(crate) fn removed(&self) {
        let mut out = lock(&self.flight);
        out.generation += 1;
        out.flight = None;
    }
}

/// One snapshot thread's outcome, as it stands.
#[derive(Default)]
struct Flight {
    outcome: Mutex<Outcome>,
    ended: Condvar,
}

#[derive(Default)]
enum Outcome {
    #[default]
    Running,
    Answered(Snapshot),
    /// The thread ended with no answer (it panicked).
    Lost,
}

impl Flight {
    fn running(&self) -> bool {
        matches!(*lock(&self.outcome), Outcome::Running)
    }

    fn end(&self, outcome: Outcome) {
        *lock(&self.outcome) = outcome;
        self.ended.notify_all();
    }

    /// The outcome once it came, waiting at most `wait`.
    fn wait(&self, wait: Duration) -> Result<Snapshot, NoAnswer> {
        let guard = lock(&self.outcome);
        let (guard, _timed_out) = self
            .ended
            .wait_timeout_while(guard, wait, |outcome| matches!(outcome, Outcome::Running))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &*guard {
            Outcome::Answered(snapshot) => Ok(snapshot.clone()),
            Outcome::Running => Err(NoAnswer("timed_out")),
            Outcome::Lost => Err(NoAnswer("ended")),
        }
    }
}

/// Ends its flight [`Outcome::Lost`] if the thread unwinds before answering.
struct Answer(Option<Arc<Flight>>);

impl Answer {
    fn send(mut self, snapshot: Snapshot) {
        if let Some(flight) = self.0.take() {
            flight.end(Outcome::Answered(snapshot));
        }
    }
}

impl Drop for Answer {
    fn drop(&mut self) {
        if let Some(flight) = self.0.take() {
            flight.end(Outcome::Lost);
        }
    }
}

/// A snapshot of `sandbox`, waiting at most `wait`: the one out if one is, else a new thread's.
pub(crate) fn take(
    sandbox: &Sandbox,
    data_dir: &Path,
    wait: Duration,
) -> Result<Snapshot, NoAnswer> {
    let flight = {
        let mut out = lock(&sandbox.bounds.slot.flight);
        let generation = out.generation;
        match &out.flight {
            Some((started, flight)) if *started == generation && flight.running() => {
                Arc::clone(flight)
            }
            _ => {
                let flight = Arc::new(Flight::default());
                start(sandbox, data_dir.to_path_buf(), Arc::clone(&flight))?;
                out.flight = Some((generation, Arc::clone(&flight)));
                flight
            }
        }
    };
    flight.wait(wait)
}

/// Starts the thread that reads `sandbox` into `flight`.
fn start(sandbox: &Sandbox, data_dir: PathBuf, flight: Arc<Flight>) -> Result<(), NoAnswer> {
    #[cfg(test)]
    sandbox
        .bounds
        .slot
        .started
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let detect = Arc::clone(&sandbox.detect);
    let setup = sandbox.setup.clone();
    let answer = Answer(Some(flight));
    std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let detection = detect();
            let state = setup.as_ref().map(|setup| setup.state());
            let set_up_at = match state {
                Some(SetupState::Ready) => super::set_up_at::read(&data_dir),
                _ => None,
            };
            answer.send(Snapshot {
                detection,
                state,
                set_up_at,
            });
        })
        .map(drop)
        .map_err(|_refused| NoAnswer("no_thread"))
}
