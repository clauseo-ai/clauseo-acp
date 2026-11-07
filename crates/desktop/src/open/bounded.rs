//! Where `open_file` and `reveal` do their blocking work (the marker check, `canonicalize`,
//! `metadata`, `is_dir`, the short-name lookup and the OS's open or reveal): on a thread of its
//! own, waited on for at most a bound, so a folder on a share that does not answer never holds
//! an async runtime thread. As `matters/probe.rs` does in the host (#617), with the same 10 s.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use clauseo_protocol::{ErrorCode, HostError};
use futures::channel::oneshot;

/// How long the work of one call waits for the file system: #617's bound, as the host's
/// `PROBE_WAIT`.
pub(crate) const FOLDER_WAIT: Duration = Duration::from_secs(10);

/// The message of a call whose folder did not answer within the bound. No path.
pub(crate) const NO_ANSWER: &str = "the folder did not answer in time";

/// Runs `work` on a thread of its own and answers what it answers, waiting at most `bound`
/// without blocking the caller's thread. Past the bound the answer is `late` (for a matter's
/// folder `matter.folder_missing`, retryable) with [`NO_ANSWER`], and the thread is left to
/// finish on its own, its answer dropped. A thread that does not start, or ends without an
/// answer (it panicked), is `internal`. `what` names the work in the log.
pub(crate) async fn bounded<T: Send + 'static>(
    what: &'static str,
    bound: Duration,
    late: ErrorCode,
    work: impl FnOnce() -> Result<T, HostError> + Send + 'static,
) -> Result<T, HostError> {
    let (answer, answered) = oneshot::channel();
    let waiter = std::thread::Builder::new()
        .name("clauseo-open-wait".to_owned())
        .spawn(move || {
            let (send, done) = mpsc::sync_channel(1);
            let worker = std::thread::Builder::new()
                .name("clauseo-open".to_owned())
                .spawn(move || {
                    let _gone = send.send(work());
                });
            let result = match worker.map(|_| done.recv_timeout(bound)) {
                Ok(Ok(result)) => result,
                Ok(Err(RecvTimeoutError::Timeout)) => {
                    tracing::warn!(
                        what,
                        bound_ms = bound.as_millis(),
                        "the folder did not answer"
                    );
                    Err(HostError::new(late, NO_ANSWER))
                }
                Ok(Err(RecvTimeoutError::Disconnected)) => Err(lost(what)),
                Err(error) => Err(HostError::new(
                    ErrorCode::Internal,
                    format!("the {what} thread did not start: {error}"),
                )),
            };
            let _gone = answer.send(result);
        });
    if let Err(error) = waiter {
        return Err(HostError::new(
            ErrorCode::Internal,
            format!("the {what} thread did not start: {error}"),
        ));
    }
    answered.await.unwrap_or_else(|_| Err(lost(what)))
}

/// The failure of work whose thread ended without an answer.
fn lost(what: &str) -> HostError {
    HostError::new(
        ErrorCode::Internal,
        format!("the {what} thread ended without an answer"),
    )
}
