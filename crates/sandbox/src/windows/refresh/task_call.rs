//! The lawyer's Task Scheduler calls, each bounded to [`TASK_CALL_LIMIT`]: the outer runner's
//! check of the stop and sweep tasks before the pool user's logon, its requests for them and for
//! the refresh task (`runner::unclean`, `runner::outer::end`), and the readiness check
//! ([`ready_within`], `UserSetup::state`), local name lookups included. A COM call to a Task
//! Scheduler that hangs, or a lookup that waits on a domain controller, would otherwise hold the
//! launch until the host's 180 s, or Settings for as long as it hangs. The call runs on its own
//! thread; one still running at the limit is left to finish there. At most one runs per kind
//! ([`Slot`]): while one is pending, a new call of that kind waits on the same answer within its
//! own limit and starts no thread. A check that didn't answer is couldn't-check
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

/// One kind of call: the one still running, if any, which a new call of that kind waits on
/// instead of starting another thread.
pub(crate) struct Slot<T> {
    /// The call running now; its worker clears it once it ends.
    pending: Mutex<Option<Arc<Pending<T>>>>,
    /// Worker threads started, for the tests.
    started: AtomicUsize,
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
    /// The call panicked.
    Lost,
}

impl<T> Slot<T> {
    /// No call running.
    pub(crate) const fn new() -> Slot<T> {
        Slot {
            pending: Mutex::new(None),
            started: AtomicUsize::new(0),
        }
    }

    /// How many worker threads this kind has started.
    #[cfg(test)]
    pub(crate) fn started(&self) -> usize {
        self.started.load(Ordering::Relaxed)
    }
}

/// Ends a worker: its answer, or [`Answer::Lost`] when it panicked, told to every waiter, and
/// its slot cleared, so the next call of the kind starts afresh.
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
        let mut slot = self
            .slot
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot
            .as_ref()
            .is_some_and(|running| Arc::ptr_eq(running, &self.pending))
        {
            *slot = None;
        }
        drop(slot);
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
    let pending = {
        let mut running = slot.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(pending) = running.as_ref() {
            Arc::clone(pending)
        } else {
            let pending = Arc::new(Pending {
                answer: Mutex::new(Answer::Running),
                answered: Condvar::new(),
            });
            let finish = Finish {
                slot,
                pending: Arc::clone(&pending),
            };
            std::thread::Builder::new()
                .name("clauseo-task-call".to_owned())
                .spawn(move || {
                    let answer = call();
                    *finish
                        .pending
                        .answer
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = Answer::Answered(answer);
                    drop(finish);
                })
                .map_err(|error| {
                    format!("the Task Scheduler call's thread didn't start ({error})")
                })?;
            slot.started.fetch_add(1, Ordering::Relaxed);
            *running = Some(Arc::clone(&pending));
            pending
        }
    };
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
        Answer::Running => Err(format!(
            "Windows Task Scheduler didn't answer within {} s",
            limit.as_secs()
        )),
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
