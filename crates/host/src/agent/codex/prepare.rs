//! A launch's file system checks and writes, off the actor: every agent's data-folder refusal,
//! a Codex agent's launch preparation ([`super::prepare_launch`]) and its `.codex` re-check
//! before each prompt and session call.
//! As `actor/folder_state.rs` does for matter folders, each runs on a `clauseo-codex-fs`
//! thread that sends its answer back over a channel, and the caller waits at most
//! [`PREPARE_WAIT`] on the host's clock. A stat that hangs (a network share that does not
//! answer) strands at most that thread, and the launch or the turn fails closed.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::ErrorCode;
use futures::future::{Either, select};

use crate::clock::Clock;
use crate::error::Error;

/// How long a launch or a Codex turn waits for its file system work.
pub(crate) const PREPARE_WAIT: Duration = Duration::from_secs(5);

/// The name of the threads that do it.
const THREAD_NAME: &str = "clauseo-codex-fs";

/// Runs `work` on a new `clauseo-codex-fs` thread and gives its answer; `matter.folder_missing`
/// when it has not answered within [`PREPARE_WAIT`] on `clock` (the thread is left to finish on
/// its own), and `agent.launch_failed` when it could not start or panicked.
pub(crate) async fn off_actor<T: Send + 'static>(
    clock: Arc<dyn Clock>,
    work: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    off_actor_or_undo(clock, work, drop).await
}

/// [`off_actor`] for work that leaves something behind: when the caller has stopped waiting
/// (past [`PREPARE_WAIT`]) by the time `work` answers `Ok`, `undo` gets that answer on the same
/// thread, so what it made is taken back there and never on the actor. An answer is either
/// taken by the caller or undone, never both and never neither: the caller closes the channel
/// when it gives up, then takes an answer already sent.
pub(crate) async fn off_actor_or_undo<T: Send + 'static>(
    clock: Arc<dyn Clock>,
    work: impl FnOnce() -> Result<T, Error> + Send + 'static,
    undo: impl FnOnce(T) + Send + 'static,
) -> Result<T, Error> {
    let (send, answer) = async_channel::bounded(1);
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            // The caller stopped waiting: nobody is left to tell, so what was made is undone.
            if let Err(async_channel::SendError(Ok(made))) = send.send_blocking(work()) {
                undo(made);
            }
        });
    if let Err(error) = started {
        tracing::warn!(%error, "could not start {THREAD_NAME}");
        return Err(not_ready());
    }
    let waited = {
        let answer = std::pin::pin!(answer.recv());
        match select(answer, clock.after(PREPARE_WAIT)).await {
            Either::Left((received, _)) => Some(received),
            Either::Right(((), _)) => None,
        }
    };
    match waited {
        Some(Ok(result)) => result,
        Some(Err(_closed)) => {
            tracing::warn!("{THREAD_NAME} ended without an answer");
            Err(not_ready())
        }
        None => {
            // From here a send fails, and the thread undoes; one sent before is taken here.
            answer.close();
            if let Ok(result) = answer.try_recv() {
                return result;
            }
            tracing::warn!("the matter folder's checks did not answer in time; the call fails");
            Err(too_late())
        }
    }
}

/// The `.codex` re-check of a Codex agent's matter folder before a prompt or a session call, off
/// the actor: `stored`, the folder the agent runs in, is resolved through links again each
/// time, so a link pointed elsewhere since the launch is checked where it now points. `None`
/// (any other agent) passes at once, with no thread.
pub(crate) async fn recheck(clock: Arc<dyn Clock>, stored: Option<PathBuf>) -> Result<(), Error> {
    match stored {
        Some(stored) => {
            off_actor(clock, move || {
                super::refuse_codex_folder(&crate::agent::session_folder(&stored))
            })
            .await
        }
        None => Ok(()),
    }
}

/// The failure when the checks did not answer in time: the folder may be a share that does not
/// answer, and the agent may be running already, so not "didn't start".
fn too_late() -> Error {
    Error::refused(
        ErrorCode::MatterFolderMissing,
        "Clauseo couldn't check the matter's folder in time.",
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
