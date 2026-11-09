//! The lawyer's Task Scheduler calls, each bounded to [`TASK_CALL_LIMIT`]: the outer runner's
//! check of the stop and sweep tasks before the pool user's logon, its requests for them and for
//! the refresh task (`runner::unclean`, `runner::outer::end`), and the readiness check
//! ([`ready_within`], `UserSetup::state`), local name lookups included. A COM call to a Task
//! Scheduler that hangs, or a lookup that waits on a domain controller, would otherwise hold the
//! launch until the host's 180 s, or Settings for as long as it hangs. The call runs on its own
//! thread; one still running at the limit is left to finish there. At most one runs per kind
//! ([`Slot`]): while one is pending, a new call of that kind waits on the same answer within its
//! own limit and starts no thread; one that timed out three times in a row is abandoned, with at
//! most two abandoned per kind still running. A check that didn't answer is couldn't-check
//! ([`TASK_UNCHECKED`]), which the launch reads once more and then refuses
//! (`unclean::tasks_ready`). No Windows call here; tested on every OS.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use super::task_identity::{TASK_UNCHECKED, TaskReader, ready_with_tasks};
use crate::SetupState;

#[cfg(test)]
#[path = "task_call_tests.rs"]
mod tests;

/// How long a launch, or the readiness check, waits for one Task Scheduler call: well past a
/// healthy answer (a few RPCs), short against the host's 180 s, with the one retry included.
pub(crate) const TASK_CALL_LIMIT: Duration = Duration::from_secs(10);

/// Timeouts in a row on one call before it is abandoned and the kind may start a new worker.
pub(crate) const TIMEOUTS_BEFORE_ABANDON: u32 = 3;

/// Abandoned workers a kind may have still running; past it, no new worker starts.
pub(crate) const ABANDONED_CAP: u32 = 2;

/// Starts a worker thread running `work`; `std::thread` outside the tests.
pub(crate) type Spawn = fn(Box<dyn FnOnce() + Send>) -> std::io::Result<()>;

/// A named thread for `work`.
fn spawn_thread(work: Box<dyn FnOnce() + Send>) -> std::io::Result<()> {
    let thread = std::thread::Builder::new().name("clauseo-task-call".to_owned());
    thread.spawn(work).map(drop)
}

/// One kind of call: the one still running, if any, which a new call of that kind waits on
/// instead of starting another thread. A call that times out [`TIMEOUTS_BEFORE_ABANDON`] times
/// in a row is abandoned, so one new worker may start, while at most [`ABANDONED_CAP`] abandoned
/// ones still run.
pub(crate) struct Slot<T> {
    /// The kind, for the log.
    kind: &'static str,
    state: Mutex<SlotState<T>>,
    spawn: Spawn,
    /// Worker threads started, for the tests.
    started: AtomicUsize,
}

/// A slot's call and its count.
struct SlotState<T> {
    /// The call running now; its worker clears it once it ends.
    pending: Option<Arc<Pending<T>>>,
    /// Timeouts in a row on `pending`; an answer resets it.
    timeouts: u32,
    /// Abandoned workers still running.
    abandoned: u32,
}

/// A running call's answer, once it has one.
struct Pending<T> {
    answer: Mutex<Answer<T>>,
    answered: Condvar,
}

/// Where a running call is.
enum Answer<T> {
    Running,
    Answered(T),
    /// The call panicked, or its thread didn't start.
    Lost,
}

impl<T> Slot<T> {
    /// No call running, of the kind `kind`.
    pub(crate) const fn new(kind: &'static str) -> Slot<T> {
        Slot::with_spawn(kind, spawn_thread)
    }

    /// [`Slot::new`] starting its workers through `spawn`.
    pub(crate) const fn with_spawn(kind: &'static str, spawn: Spawn) -> Slot<T> {
        Slot {
            kind,
            state: Mutex::new(SlotState {
                pending: None,
                timeouts: 0,
                abandoned: 0,
            }),
            spawn,
            started: AtomicUsize::new(0),
        }
    }

    /// How many worker threads this kind has started.
    #[cfg(test)]
    pub(crate) fn started(&self) -> usize {
        self.started.load(Ordering::Relaxed)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SlotState<T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Ends a worker: its answer, or [`Answer::Lost`] when it panicked or never started, told to
/// every waiter, and its slot cleared, so the next call of the kind starts afresh; an abandoned
/// worker ending frees its place under [`ABANDONED_CAP`].
struct Finish<T: 'static> {
    slot: &'static Slot<T>,
    pending: Arc<Pending<T>>,
}

impl<T> Drop for Finish<T> {
    fn drop(&mut self) {
        let mut answer = self
            .pending
            .answer
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if matches!(*answer, Answer::Running) {
            *answer = Answer::Lost;
        }
        drop(answer);
        let mut state = self.slot.lock();
        if state
            .pending
            .as_ref()
            .is_some_and(|running| Arc::ptr_eq(running, &self.pending))
        {
            state.pending = None;
            state.timeouts = 0;
        } else {
            state.abandoned = state.abandoned.saturating_sub(1);
        }
        drop(state);
        self.pending.answered.notify_all();
    }
}

/// Runs `call` on its own thread, or joins the call of this kind (`slot`) still running, and
/// waits at most `limit` for the answer: `Err` when it didn't answer by then, or its thread
/// couldn't start or panicked.
pub(crate) fn within<T: Clone + Send + 'static>(
    slot: &'static Slot<T>,
    limit: Duration,
    call: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (pending, start) = {
        let mut state = slot.lock();
        if let Some(pending) = state.pending.as_ref() {
            (Arc::clone(pending), false)
        } else {
            let pending = Arc::new(Pending {
                answer: Mutex::new(Answer::Running),
                answered: Condvar::new(),
            });
            state.pending = Some(Arc::clone(&pending));
            (pending, true)
        }
    };
    // The slot's lock is free here: a thread that doesn't start drops `finish`, which takes it.
    if start {
        let finish = Finish {
            slot,
            pending: Arc::clone(&pending),
        };
        let work = move || {
            let answer = call();
            *finish
                .pending
                .answer
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Answer::Answered(answer);
            drop(finish);
        };
        if let Err(error) = (slot.spawn)(Box::new(work)) {
            return Err(format!(
                "the Task Scheduler call's thread didn't start ({error})"
            ));
        }
        slot.started.fetch_add(1, Ordering::Relaxed);
    }
    let answer = pending
        .answer
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let (answer, _timed_out) = pending
        .answered
        .wait_timeout_while(answer, limit, |answer| matches!(answer, Answer::Running))
        .unwrap_or_else(PoisonError::into_inner);
    match &*answer {
        Answer::Answered(answer) => Ok(answer.clone()),
        Answer::Lost => Err("the Task Scheduler call failed".to_owned()),
        Answer::Running => {
            drop(answer);
            timed_out(slot, &pending);
            Err(format!(
                "Windows Task Scheduler didn't answer within {} s",
                limit.as_secs()
            ))
        }
    }
}

/// Counts a timeout on `pending`; at [`TIMEOUTS_BEFORE_ABANDON`] in a row, abandons it so the
/// next call starts a new worker, unless [`ABANDONED_CAP`] abandoned ones still run.
fn timed_out<T>(slot: &Slot<T>, pending: &Arc<Pending<T>>) {
    let mut state = slot.lock();
    let current = state.pending.as_ref();
    if !current.is_some_and(|running| Arc::ptr_eq(running, pending)) {
        return;
    }
    state.timeouts = state.timeouts.saturating_add(1);
    if state.timeouts >= TIMEOUTS_BEFORE_ABANDON && state.abandoned < ABANDONED_CAP {
        state.pending = None;
        state.timeouts = 0;
        state.abandoned = state.abandoned.saturating_add(1);
        tracing::warn!(kind = slot.kind, "a Task Scheduler call was abandoned");
    }
}

/// The launch's check of the tasks (`refresh::tasks_repair`) within `limit`: what it says, or
/// [`TASK_UNCHECKED`] when it didn't answer.
pub(crate) fn repair_within(
    slot: &'static Slot<Option<&'static str>>,
    limit: Duration,
    read: impl FnOnce() -> Option<&'static str> + Send + 'static,
) -> Option<&'static str> {
    within(slot, limit, read).unwrap_or_else(|why| {
        tracing::warn!(%why, "the sandbox's tasks can't be checked");
        Some(TASK_UNCHECKED)
    })
}

/// The readiness check's last step (`task_identity::ready_with_tasks`) over the reader `reader`
/// makes, within `limit`, Task Scheduler reads and name lookups together: a `Ready` whose check
/// didn't answer by then is [`SetupState::Failed`] in [`TASK_UNCHECKED`]'s words. It only reads:
/// a missing or changed task is said, and put back by a launch's heal or the repair, never here.
pub(crate) fn ready_within<R: TaskReader>(
    slot: &'static Slot<SetupState>,
    limit: Duration,
    state: SetupState,
    reader: impl FnOnce() -> R + Send + 'static,
) -> SetupState {
    if state != SetupState::Ready {
        return state;
    }
    let check = move || ready_with_tasks(SetupState::Ready, &mut reader());
    within(slot, limit, check).unwrap_or_else(|why| {
        tracing::warn!(%why, "the sandbox's tasks can't be checked");
        SetupState::Failed(TASK_UNCHECKED.to_owned())
    })
}
