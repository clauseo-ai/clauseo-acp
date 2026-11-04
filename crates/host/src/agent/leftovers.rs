//! What a cancelled turn left running. When a turn's prompt goes out, the agent's descendants
//! are listed ([`snapshot`]); when the turn ends cancelled, every descendant new since then
//! that started before the turn's end is ended with what it started ([`sweep`]), but for the
//! session's MCP servers: an agent can keep
//! a command running past a cancel (Codex keeps each background terminal on `turn/interrupt`),
//! and Stop must end what the turn started. The agent itself and what ran before the prompt
//! stay, and so does whatever a later turn starts. Both run on a `clauseo-sweep` thread, never
//! on the actor: on Linux a listing reads `/proc/<pid>/stat` of every process, and the sweep
//! waits 2 s between its passes. An agent's stop lists the same way before its ladder.

use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use clauseo_process::{Cutoff, Descendants, Snapshot, Swept};
use futures::future::{Either, select};

use crate::clock::{Clock, ProcessTimer};

/// The name of the threads that list and sweep.
const THREAD_NAME: &str = "clauseo-sweep";

/// How long a prompt waits for its listing before it goes out without one, and so with no sweep
/// should it be cancelled. A listing takes a few milliseconds.
pub(crate) const SNAPSHOT_WAIT: Duration = Duration::from_secs(1);

/// The agent's descendants now, listed on a `clauseo-sweep` thread; `None` when the thread could
/// not start or did not answer within [`SNAPSHOT_WAIT`] on `clock`.
pub(crate) async fn snapshot(descendants: Descendants, clock: Arc<dyn Clock>) -> Option<Snapshot> {
    let (send, answer) = async_channel::bounded(1);
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            // The prompt stopped waiting: nobody is left to take the listing.
            drop(send.send_blocking(descendants.snapshot()));
        });
    if let Err(error) = started {
        tracing::warn!(%error, "could not start {THREAD_NAME} for a listing");
        return None;
    }
    let answer = pin!(answer.recv());
    match select(answer, clock.after(SNAPSHOT_WAIT)).await {
        Either::Left((Ok(snapshot), _)) => Some(snapshot),
        Either::Left((Err(async_channel::RecvError), _)) | Either::Right(((), _)) => {
            tracing::warn!("the agent's descendants were not listed; a cancel ends none");
            None
        }
    }
}

/// Ends, on a `clauseo-sweep` thread, every descendant of the agent new since `before` that
/// started before `cutoff`, with what those start meanwhile, but those running one of `keep`
/// (the session's MCP server programs, compared as given and resolved through links), with the
/// 2 s between the passes on `clock`. Returns at once.
pub(crate) fn sweep(
    descendants: Descendants,
    before: Snapshot,
    cutoff: Cutoff,
    keep: Vec<PathBuf>,
    clock: Arc<dyn Clock>,
) {
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let keep = resolved(keep);
            let timer = ProcessTimer(clock);
            let swept =
                futures::executor::block_on(descendants.end_new(&before, cutoff, &keep, &timer));
            match swept {
                Swept::Ended { processes } => {
                    tracing::info!(processes, "ended what a cancelled turn left running");
                }
                Swept::NotListed => {
                    tracing::debug!("the agent's descendants were not listed; none was ended");
                }
                Swept::NothingNew => {}
            }
        });
    if let Err(error) = started {
        tracing::warn!(%error, "could not start {THREAD_NAME}; a cancelled turn's processes run on");
    }
}

/// Each of `keep`, and its canonical path when that differs, as a running executable is named.
fn resolved(keep: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(keep.len() * 2);
    for path in keep {
        if let Ok(canonical) = dunce::canonicalize(&path)
            && canonical != path
        {
            paths.push(canonical);
        }
        paths.push(path);
    }
    paths
}
