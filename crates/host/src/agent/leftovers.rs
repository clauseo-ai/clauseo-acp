//! What a cancelled turn left running. When a turn's prompt goes out, the agent's descendants
//! are listed ([`snapshot`]); when the turn ends cancelled, every descendant new since then
//! that started before the turn's end is ended with what it started ([`sweep`]), but for the
//! session's MCP servers: an agent can keep
//! a command running past a cancel (Codex keeps each background terminal on `turn/interrupt`),
//! and Stop must end what the turn started. The agent itself and what ran before the prompt
//! stay, and so does whatever a later turn starts. Both run on a `clauseo-sweep` thread, never
//! on the actor: on Linux a listing reads `/proc/<pid>/stat` of every process, and the sweep
//! waits 2 s between its passes. An agent's stop lists the same way before its ladder, waiting
//! longer ([`STOP_LIST_WAIT`]); a listing later than that is ended when it comes ([`end_late`]).
//! A prompt's listing later than its wait is kept too: the prompt goes out without it, and a
//! cancel's sweep waits for it ([`LATE_LIST_WAIT`]). One that comes late may have listed what the
//! turn started meanwhile, which the sweep then spares.

use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use clauseo_process::{Cutoff, Descendants, Snapshot, Swept};
use futures::future::{Either, select};

use crate::clock::{Clock, ProcessTimer};

/// The name of the threads that list and sweep.
const THREAD_NAME: &str = "clauseo-sweep";

/// How long a prompt waits for its listing before it goes out without one, and so with no sweep
/// should it be cancelled. A listing takes a few milliseconds.
pub(crate) const SNAPSHOT_WAIT: Duration = Duration::from_secs(1);

/// The listings started and not yet taken, in this process (the tests' dump of the stop path).
static LISTING: AtomicUsize = AtomicUsize::new(0);

/// How many listings run now, in this process.
#[cfg(feature = "test-support")]
pub(crate) fn listings_running() -> usize {
    LISTING.load(Ordering::SeqCst)
}

/// How long an agent's stop waits for its listing before its ladder: a listing after the
/// ladder's group kill walks a tree whose commands the agent's death has re-parented away.
pub(crate) const STOP_LIST_WAIT: Duration = Duration::from_secs(5);

/// How long a stop's listing that missed [`STOP_LIST_WAIT`] is still waited for, on a
/// `clauseo-sweep` thread after the ladder, to end what it found.
pub(crate) const LATE_LIST_WAIT: Duration = Duration::from_secs(30);

/// A listing held after it is taken: it says so on the first channel, then waits for the second
/// to close.
#[cfg(test)]
type Hold = (async_channel::Sender<()>, async_channel::Receiver<()>);

#[cfg(test)]
thread_local! {
    /// What holds the next listing started from this thread.
    static HOLD_NEXT: std::cell::RefCell<Option<Hold>> = const { std::cell::RefCell::new(None) };
}

/// A slow listing, for tests: the next one started from this thread, once taken, sends on
/// `taken` and answers only when `release` drops.
#[cfg(test)]
pub(crate) struct HeldListing {
    pub(crate) taken: async_channel::Receiver<()>,
    /// Dropped (with the whole hold) to let the listing answer.
    pub(crate) _release: async_channel::Sender<()>,
}

/// Holds the next listing started from this thread ([`HeldListing`]).
#[cfg(test)]
pub(crate) fn hold_next_listing() -> HeldListing {
    let (said, taken) = async_channel::bounded(1);
    let (release, held) = async_channel::bounded(1);
    HOLD_NEXT.with(|hold| *hold.borrow_mut() = Some((said, held)));
    HeldListing {
        taken,
        _release: release,
    }
}

/// A listing of the agent's descendants: taken within its wait, or still coming, or none.
#[derive(Debug)]
pub(crate) enum Listed {
    Taken(Snapshot),
    /// Not back within its wait: kept, and waited for again where it is used, up to
    /// [`LATE_LIST_WAIT`].
    Late(async_channel::Receiver<Snapshot>),
    /// The thread could not start, or answered nothing.
    Failed,
}

/// The agent's descendants now, listed on a `clauseo-sweep` thread, waiting up to `wait` on
/// `clock`: a prompt waits [`SNAPSHOT_WAIT`], a stop [`STOP_LIST_WAIT`].
pub(crate) async fn listing(
    descendants: Descendants,
    clock: Arc<dyn Clock>,
    wait: Duration,
) -> Listed {
    let Some(answer) = start_listing(descendants) else {
        return Listed::Failed;
    };
    match within(&answer, &*clock, wait).await {
        Some(snapshot) => Listed::Taken(snapshot),
        // Kept late on Unix only: Windows waits as before (its stop ends the whole job).
        None if cfg!(not(unix)) || (answer.is_closed() && answer.is_empty()) => {
            tracing::warn!("the agent's descendants were not listed in time");
            Listed::Failed
        }
        None => {
            tracing::warn!("the agent's descendants were not listed in time; kept for later");
            Listed::Late(answer)
        }
    }
}

/// `listed`'s snapshot, waiting for a late one up to [`LATE_LIST_WAIT`] on `clock`. Blocks:
/// for a `clauseo-sweep` thread.
fn taken(listed: Listed, clock: &dyn Clock) -> Option<Snapshot> {
    match listed {
        Listed::Taken(snapshot) => Some(snapshot),
        Listed::Late(late) => futures::executor::block_on(within(&late, clock, LATE_LIST_WAIT)),
        Listed::Failed => None,
    }
}

/// Ends, on a `clauseo-sweep` thread, what a stop's late listing holds, once it comes within
/// [`LATE_LIST_WAIT`] on `clock`; each process only if it still runs as listed. Returns at once.
#[cfg(unix)]
pub(crate) fn end_late(late: Listed, clock: Arc<dyn Clock>) {
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            if let Some(snapshot) = taken(late, &*clock) {
                let processes = snapshot.kill_listed();
                tracing::info!(processes, "ended what a stop's late listing found");
            } else {
                tracing::warn!("a stop's listing never came; what left the group runs on");
            }
        });
    if let Err(error) = started {
        tracing::warn!(
            error = %crate::log_file::Cause(&error),
            "could not start {THREAD_NAME} for a late listing"
        );
    }
}

/// Starts the listing on a `clauseo-sweep` thread; `None` when the thread could not start.
fn start_listing(descendants: Descendants) -> Option<async_channel::Receiver<Snapshot>> {
    let (send, answer) = async_channel::bounded(1);
    #[cfg(test)]
    let hold = HOLD_NEXT.with(|hold| hold.borrow_mut().take());
    LISTING.fetch_add(1, Ordering::SeqCst);
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let snapshot = descendants.snapshot();
            LISTING.fetch_sub(1, Ordering::SeqCst);
            #[cfg(test)]
            if let Some((said, held)) = hold {
                said.send_blocking(()).unwrap_or_default();
                held.recv_blocking().unwrap_or_default();
            }
            tracing::debug!("the agent's descendants are listed");
            // The waiter gave up: nobody is left to take the listing.
            drop(send.send_blocking(snapshot));
        });
    if let Err(error) = started {
        LISTING.fetch_sub(1, Ordering::SeqCst);
        tracing::warn!(
            error = %crate::log_file::Cause(&error),
            "could not start {THREAD_NAME} for a listing"
        );
        return None;
    }
    Some(answer)
}

/// The listing on `answer` if it comes within `wait` on `clock`.
async fn within(
    answer: &async_channel::Receiver<Snapshot>,
    clock: &dyn Clock,
    wait: Duration,
) -> Option<Snapshot> {
    let answer = pin!(answer.recv());
    match select(answer, clock.after(wait)).await {
        Either::Left((Ok(snapshot), _)) => Some(snapshot),
        Either::Left((Err(async_channel::RecvError), _)) | Either::Right(((), _)) => None,
    }
}

/// Ends, on a `clauseo-sweep` thread, every descendant of the agent new since `before` (a late
/// listing waited for up to [`LATE_LIST_WAIT`]) that
/// started before `cutoff`, with what those start meanwhile, but those running one of `keep`
/// (the session's MCP server programs, compared as given and resolved through links), with the
/// 2 s between the passes on `clock`. Returns at once.
pub(crate) fn sweep(
    descendants: Descendants,
    before: Listed,
    cutoff: Cutoff,
    keep: Vec<PathBuf>,
    clock: Arc<dyn Clock>,
) {
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            // A prompt's listing that came late is waited for here, so a cancel still has it.
            let Some(before) = taken(before, &*clock) else {
                tracing::warn!("the prompt's listing never came; a cancel ends none");
                return;
            };
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
        tracing::warn!(
            error = %crate::log_file::Cause(&error),
            "could not start {THREAD_NAME}; a cancelled turn's processes run on"
        );
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
