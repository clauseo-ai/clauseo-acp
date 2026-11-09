//! The launch path's Task Scheduler calls, each bounded to [`TASK_CALL_LIMIT`]: the outer
//! runner's check of the stop and sweep tasks before the pool user's logon, and its requests for
//! them (`runner::unclean`, `runner::outer::end`). A COM call to a Task Scheduler that hangs
//! would otherwise hold the launch until the host's 180 s. The call runs on its own thread; one
//! still running at the limit is left to finish there and its answer dropped. A check that
//! didn't answer is couldn't-check ([`TASK_UNCHECKED`]), which the launch reads once more and
//! then refuses (`unclean::tasks_ready`). No Windows call here; tested on every OS.

use std::sync::mpsc;
use std::time::Duration;

use super::task_identity::TASK_UNCHECKED;

/// How long a launch waits for one Task Scheduler call: well past a healthy answer (a few RPCs),
/// short against the host's 180 s, with the one retry included.
pub(crate) const TASK_CALL_LIMIT: Duration = Duration::from_secs(10);

/// Runs `call` on its own thread and waits at most `limit` for it: `Err` when it didn't answer
/// by then, or its thread couldn't start or panicked.
pub(crate) fn within<T: Send + 'static>(
    limit: Duration,
    call: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (answer, answered) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("clauseo-task-call".to_owned())
        .spawn(move || {
            // The waiter may have given up; its answer is then dropped.
            drop(answer.send(call()));
        })
        .map_err(|error| format!("the Task Scheduler call's thread didn't start ({error})"))?;
    answered.recv_timeout(limit).map_err(|_late_or_lost| {
        format!(
            "Windows Task Scheduler didn't answer within {} s",
            limit.as_secs()
        )
    })
}

/// The launch's check of the tasks (`refresh::tasks_repair`) within `limit`: what it says, or
/// [`TASK_UNCHECKED`] when it didn't answer.
pub(crate) fn repair_within(
    limit: Duration,
    read: impl FnOnce() -> Option<&'static str> + Send + 'static,
) -> Option<&'static str> {
    within(limit, read).unwrap_or_else(|why| {
        tracing::warn!(%why, "the sandbox's tasks can't be checked");
        Some(TASK_UNCHECKED)
    })
}
