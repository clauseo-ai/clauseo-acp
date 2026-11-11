//! A failed try's temp folder, removed once npm's tree has ended: on Windows a sharing or lock
//! error (an antivirus holding a file npm just unpacked) is retried after each of
//! [`REMOVE_WAITS`], on the job's timer and through a pause (the job is already ending). A folder
//! that still will not go is logged and left for the next start's clean-up
//! ([`crate::receipt::scan`]).

use std::path::Path;
use std::time::Duration;

use clauseo_process::Timer;
use clauseo_protocol::AgentId;

use super::{Ended, in_place, windows_busy};
use crate::install::Job;
use crate::platform::Os;
use crate::write;

/// The waits before each retry of the removal after a sharing or lock error: 2 s in all. Lengths
/// no other wait of the install uses, so a test's timer can tell them.
pub(super) const REMOVE_WAITS: [Duration; 4] = [
    Duration::from_millis(200),
    Duration::from_millis(400),
    Duration::from_millis(600),
    Duration::from_millis(800),
];

/// Removes `tmp`, the temp folder of a try that failed, was paused or ran out of time, as the
/// module says.
pub(super) async fn remove_temp<T: Timer + Send + Sync>(job: &Job<T>, tmp: &Path) {
    remove_retried(
        &job.ticket.agent,
        job.config.platform.os,
        tmp,
        async |pause| job.timer.after(pause).await,
        || write::remove_dir_all(tmp),
    )
    .await;
}

/// [`remove_temp`]'s retries over `remove`, `wait` pausing between them. The last error is
/// logged (warn) with its OS code (`os error N`) and the folder's name, which with the agent
/// names the path (`<data>/agents/<agent>/install/<name>`) without the user's own folders.
pub(super) async fn remove_retried(
    agent: &AgentId,
    os: Os,
    tmp: &Path,
    wait: impl AsyncFn(Duration),
    mut remove: impl FnMut() -> std::io::Result<()>,
) {
    let removed = in_place(
        "the temp folder's removal",
        agent,
        &REMOVE_WAITS,
        async |pause| {
            wait(pause).await;
            true
        },
        async || remove(),
        |err: &std::io::Error| windows_busy(err, os),
    )
    .await;
    let Err(Ended::Failed(err)) = removed else {
        return;
    };
    let folder = tmp
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    tracing::warn!(
        %agent,
        folder = %folder,
        error = %err,
        "agent install: the temp folder could not be removed; the next start removes it"
    );
}
