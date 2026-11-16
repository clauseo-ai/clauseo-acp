//! What a cancelled turn left running. When a turn's prompt goes out, the agent's descendants
//! are listed ([`snapshot`]); when the turn ends cancelled, every descendant new since then
//! that started before the turn's end is ended with what it started ([`sweep`]), but for the
//! session's MCP servers: an agent can keep
//! a command running past a cancel (Codex keeps each background terminal on `turn/interrupt`),
//! and Stop must end what the turn started. The agent itself and what ran before the prompt
//! stay, and so does whatever a later turn starts. Both run on a `clauseo-sweep` thread, never
//! on the actor: on Linux a listing reads `/proc/<pid>/stat` of every process, and the sweep
//! waits 2 s between its passes. An agent's stop lists the same way before its ladder.
//!
//! What a session left running ends with the session: every launch gets its own marker
//! ([`session_mark`], `CLAUSEO_SESSION` in its environment), and once the agent is gone, stopped
//! or exited, [`end_session`] kills every process of this user that still carries it, on a
//! `clauseo-sweep` thread (`clauseo_process::SessionMark::sweep`), whatever its session, group
//! or parent. Turns never run it; only the agent's end does.

use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use clauseo_process::{
    AsyncIoTimer, Cutoff, Descendants, SessionMark, SessionSwept, Snapshot, Swept,
};
use clauseo_protocol::ErrorCode;
use futures::future::{Either, select};

use crate::clock::{Clock, ProcessTimer};
use crate::error::Error;

/// The name of the threads that list and sweep.
const THREAD_NAME: &str = "clauseo-sweep";

/// How long a prompt waits for its listing before it goes out without one, and so with no sweep
/// should it be cancelled. A listing takes a few milliseconds.
pub(crate) const SNAPSHOT_WAIT: Duration = Duration::from_secs(1);

/// How long, in real time, an agent's end waits for its session's sweep, which bounds itself
/// at five passes and 1 s of waits; past it the agent's end goes on and the sweep finishes alone.
const SESSION_SWEEP_WAIT: Duration = Duration::from_secs(5);

/// The listings started and not yet taken, in this process (the tests' dump of the stop path).
static LISTING: AtomicUsize = AtomicUsize::new(0);

/// How many listings run now, in this process.
#[cfg(feature = "test-support")]
pub(crate) fn listings_running() -> usize {
    LISTING.load(Ordering::SeqCst)
}

/// The agent's descendants now, listed on a `clauseo-sweep` thread; `None` when the thread could
/// not start or did not answer within [`SNAPSHOT_WAIT`] on `clock`.
pub(crate) async fn snapshot(descendants: Descendants, clock: Arc<dyn Clock>) -> Option<Snapshot> {
    let (send, answer) = async_channel::bounded(1);
    LISTING.fetch_add(1, Ordering::SeqCst);
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let snapshot = descendants.snapshot();
            LISTING.fetch_sub(1, Ordering::SeqCst);
            tracing::debug!("the agent's descendants are listed");
            // The prompt stopped waiting: nobody is left to take the listing.
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
        tracing::warn!(
            error = %crate::log_file::Cause(&error),
            "could not start {THREAD_NAME}; a cancelled turn's processes run on"
        );
    }
}

/// A new session's marker, from 16 random bytes. A launch with no random bytes is refused: a
/// session its end could not sweep must not start.
pub(crate) fn session_mark() -> Result<SessionMark, Error> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| {
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            format!("no random bytes for the session's marker: {error}"),
        )
    })?;
    Ok(SessionMark::from_random(bytes))
}

/// Kills, on a `clauseo-sweep` thread, every process of this user that carries `mark`, and
/// returns once that sweep is done or [`SESSION_SWEEP_WAIT`] of real time has passed. Its own
/// waits are real time too ([`AsyncIoTimer`]), never the host's clock, so a test's clock that
/// stands still cannot hold an agent's end.
pub(crate) async fn end_session(mark: SessionMark) {
    let (send, answer) = async_channel::bounded(1);
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let swept = futures::executor::block_on(mark.sweep(&AsyncIoTimer));
            // The agent's end stopped waiting: nobody is left to take the answer.
            if send.send_blocking(swept).is_err() {
                tracing::debug!("the session's sweep ended after its wait");
            }
        });
    if let Err(error) = started {
        tracing::warn!(
            error = %crate::log_file::Cause(&error),
            "could not start {THREAD_NAME}; a session's leftover processes run on"
        );
        return;
    }
    let answer = pin!(answer.recv());
    let bound = pin!(async_io::Timer::after(SESSION_SWEEP_WAIT));
    match select(answer, bound).await {
        Either::Left((Ok(swept), _)) => log_swept(swept),
        Either::Left((Err(async_channel::RecvError), _)) => {
            tracing::warn!("the session's sweep ended without an answer");
        }
        Either::Right(_) => tracing::warn!("the session's sweep is still running; going on"),
    }
}

/// Logs what a session's sweep did: counts only, never a pid or a marker.
fn log_swept(swept: SessionSwept) {
    match swept {
        SessionSwept::Swept {
            ended: 0,
            left: false,
        }
        | SessionSwept::NotNeeded => {}
        SessionSwept::Swept { ended, left: false } => {
            tracing::info!(processes = ended, "ended what the session left running");
        }
        SessionSwept::Swept { ended, left: true } => tracing::warn!(
            processes = ended,
            "the session's sweep ran out of passes with a marked process left"
        ),
        SessionSwept::NotListed => {
            tracing::warn!("the processes could not be listed; the session's sweep ended none");
        }
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
