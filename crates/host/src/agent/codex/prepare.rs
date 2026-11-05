//! File system work that may hang, run off the actor: a launch's checks and writes (every
//! agent's data-folder refusal, a Codex agent's launch preparation, [`super::prepare_launch`]),
//! a Codex agent's `.codex` re-check before each prompt and session call, and the folder check
//! of a folder picked for a new matter (`actor/query/folder_support.rs`).
//! As `actor/folder_state.rs` does for matter folders, each runs on a `clauseo-codex-fs`
//! thread that sends its answer back over a channel, and the caller waits at most its
//! [`Job::wait`] on the host's clock: [`LAUNCH_WAIT`] for a launch's preparation, which writes
//! several files, [`PREPARE_WAIT`] for the rest. A stat that hangs (a network share that does
//! not answer) strands at most that thread, and the caller fails closed. Waiting too long is
//! not a missing folder: [`off_actor`] fails as `agent.launch_failed`, which may be retried
//! (the picked-folder query answers `check_failed` instead), and the log says what was waited
//! on, for how long, and how it ended once it did.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::{ErrorCode, MatterId};
use futures::future::{Either, select};

use crate::clock::Clock;
use crate::error::Error;

/// How long a Codex turn's `.codex` re-check, and a run folder's removal, wait for their file
/// system work.
pub(crate) const PREPARE_WAIT: Duration = Duration::from_secs(5);

/// How long a launch waits for its preparation: the matter folder's checks, then Codex's home
/// and `config.toml`, the run folders, agent-browser's config and the skills, each written on
/// the first launch. On Windows an antivirus scan of each new file can make that take longer
/// than [`PREPARE_WAIT`] (clauseo-ai/clauseo-acp-private#365: a first Codex launch failed
/// there and the same message sent again worked), so it gets more time, still bounded.
pub(crate) const LAUNCH_WAIT: Duration = Duration::from_secs(30);

/// A call off the actor: what it does and for which matter, for the log, and how long its caller
/// waits for it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Job {
    /// What it does, in a few words.
    pub(crate) what: &'static str,
    /// The matter it works for, when known: the log names no path.
    pub(crate) matter: Option<MatterId>,
    /// How long the caller waits.
    pub(crate) wait: Duration,
}

impl Job {
    /// A launch's preparation for `matter`, waited on for [`LAUNCH_WAIT`].
    pub(crate) const fn launch(matter: MatterId) -> Self {
        Self {
            what: "launch preparation",
            matter: Some(matter),
            wait: LAUNCH_WAIT,
        }
    }

    /// Other work, waited on for [`PREPARE_WAIT`].
    pub(crate) const fn short(what: &'static str, matter: Option<MatterId>) -> Self {
        Self {
            what,
            matter,
            wait: PREPARE_WAIT,
        }
    }
}

/// The name of the threads that do it.
const THREAD_NAME: &str = "clauseo-codex-fs";

/// Runs `work` on a new `clauseo-codex-fs` thread and gives its answer; `agent.launch_failed`
/// when it has not answered within `job`'s wait on `clock` (the thread is left to finish on its
/// own), when it could not start, or when it panicked.
pub(crate) async fn off_actor<T: Send + 'static>(
    clock: Arc<dyn Clock>,
    job: Job,
    work: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    off_actor_or_undo(clock, job, work, drop).await
}

/// [`off_actor`] for work that leaves something behind: when the caller has stopped waiting
/// (past `job`'s wait) by the time `work` answers `Ok`, `undo` gets that answer on the same
/// thread, so what it made is taken back there and never on the actor. An answer is either
/// taken by the caller or undone, never both and never neither: the caller closes the channel
/// when it gives up, then takes an answer already sent.
pub(crate) async fn off_actor_or_undo<T: Send + 'static>(
    clock: Arc<dyn Clock>,
    job: Job,
    work: impl FnOnce() -> Result<T, Error> + Send + 'static,
    undo: impl FnOnce(T) + Send + 'static,
) -> Result<T, Error> {
    let (send, answer) = async_channel::bounded(1);
    let thread_clock = Arc::clone(&clock);
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let start = thread_clock.now().mono;
            let Err(async_channel::SendError(late)) = send.send_blocking(work()) else {
                return;
            };
            // The caller stopped waiting: the log says how long the work really took.
            let took = thread_clock.now().mono.saturating_sub(start);
            let outcome = late
                .as_ref()
                .map_or_else(|error| error.code().as_str(), |_| "ok");
            tracing::warn!(
                what = job.what,
                matter = ?job.matter,
                took_ms = took.as_millis(),
                outcome,
                "{THREAD_NAME} answered after its caller stopped waiting"
            );
            // Nobody is left to tell, so what was made is undone.
            if let Ok(made) = late {
                undo(made);
            }
        });
    if let Err(error) = started {
        tracing::warn!(what = job.what, matter = ?job.matter, %error, "could not start {THREAD_NAME}");
        return Err(not_ready());
    }
    let waited = {
        let answer = std::pin::pin!(answer.recv());
        match select(answer, clock.after(job.wait)).await {
            Either::Left((received, _)) => Some(received),
            Either::Right(((), _)) => None,
        }
    };
    match waited {
        Some(Ok(result)) => result,
        Some(Err(_closed)) => {
            tracing::warn!(what = job.what, matter = ?job.matter, "{THREAD_NAME} ended without an answer");
            Err(not_ready())
        }
        None => {
            // From here a send fails, and the thread undoes; one sent before is taken here.
            answer.close();
            if let Ok(result) = answer.try_recv() {
                return result;
            }
            tracing::warn!(
                what = job.what,
                matter = ?job.matter,
                waited_ms = job.wait.as_millis(),
                error_kind = "timed out",
                "{THREAD_NAME} did not answer in time; the call fails and may be retried"
            );
            Err(too_late())
        }
    }
}

/// The `.codex` re-check of a Codex agent's matter folder before a prompt or a session call, off
/// the actor: `stored`, the folder the agent runs in, is resolved through links again each
/// time, so a link pointed elsewhere since the launch is checked where it now points. `None`
/// (any other agent) passes at once, with no thread. `matter` is for the log.
pub(crate) async fn recheck(
    clock: Arc<dyn Clock>,
    matter: Option<MatterId>,
    stored: Option<PathBuf>,
) -> Result<(), Error> {
    match stored {
        Some(stored) => {
            off_actor(
                clock,
                Job::short("the .codex re-check", matter),
                move || super::refuse_codex_folder(&crate::agent::session_folder(&stored)),
            )
            .await
        }
        None => Ok(()),
    }
}

/// The failure when the work did not answer in time. Not `matter.folder_missing`: the folder
/// may be there and merely slow (a first launch's files scanned by an antivirus), and telling
/// the lawyer it moved would be wrong. `agent.launch_failed` may be retried, and the lawyer's
/// question is kept for that; the log says what was slow (#365).
fn too_late() -> Error {
    Error::refused(
        ErrorCode::AgentLaunchFailed,
        "Clauseo's checks of the matter's folder did not answer in time.",
    )
}

/// The failure when the checks' thread could not start or ended without an answer.
fn not_ready() -> Error {
    Error::refused(
        ErrorCode::AgentLaunchFailed,
        "Clauseo couldn't check the matter's folder.",
    )
}

#[cfg(test)]
mod tests;
