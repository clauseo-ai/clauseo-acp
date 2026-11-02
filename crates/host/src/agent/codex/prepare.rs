//! A Codex agent's file system checks and writes, off the actor: its launch preparation
//! ([`super::prepare_launch`]) and the `.codex` re-check before each prompt and session call.
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

/// How long a Codex launch or turn waits for its file system work.
pub(crate) const PREPARE_WAIT: Duration = Duration::from_secs(5);

/// The name of the threads that do it.
const THREAD_NAME: &str = "clauseo-codex-fs";

/// Runs `work` on a new `clauseo-codex-fs` thread and gives its answer, or
/// `agent.launch_failed` when it has not answered within [`PREPARE_WAIT`] on `clock` (the
/// thread is left to finish on its own), could not start, or panicked.
pub(crate) async fn off_actor<T: Send + 'static>(
    clock: Arc<dyn Clock>,
    work: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    let (send, answer) = async_channel::bounded(1);
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            // The caller stopped waiting: nobody is left to tell.
            let _gone = send.send_blocking(work());
        });
    if let Err(error) = started {
        tracing::warn!(%error, "could not start {THREAD_NAME}");
        return Err(not_ready());
    }
    let answer = std::pin::pin!(answer.recv());
    match select(answer, clock.after(PREPARE_WAIT)).await {
        Either::Left((Ok(result), _)) => result,
        Either::Left((Err(_closed), _)) => {
            tracing::warn!("{THREAD_NAME} ended without an answer");
            Err(not_ready())
        }
        Either::Right(((), _)) => {
            tracing::warn!("Codex's file checks did not answer in time; the start fails");
            Err(not_ready())
        }
    }
}

/// The `.codex` re-check of a Codex agent's `folder` before a prompt or a session call, off the
/// actor; `None` (any other agent) passes at once, with no thread.
pub(crate) async fn recheck(clock: Arc<dyn Clock>, folder: Option<PathBuf>) -> Result<(), Error> {
    match folder {
        Some(folder) => off_actor(clock, move || super::refuse_codex_folder(&folder)).await,
        None => Ok(()),
    }
}

/// The failure when the checks did not answer.
fn not_ready() -> Error {
    Error::refused(
        ErrorCode::AgentLaunchFailed,
        "Clauseo couldn't check the matter's folder for Codex in time.",
    )
}

#[cfg(test)]
mod tests;
