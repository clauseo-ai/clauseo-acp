//! The launch's first checks, before its preparation makes anything: that the host may open the
//! matter folder, then that it is there.
//!
//! First, on macOS, the host opens the folder itself (`agent/folder_access.rs`): a folder the
//! privacy settings keep Clauseo out of is `matter.folder_permission`, and no agent starts. That
//! open is what makes macOS ask the lawyer, and it blocks until they answer, so it is waited on
//! for [`PREFLIGHT_WAIT`] (two minutes), not [`PROBE_WAIT`]: a lawyer who takes half a minute
//! to press Allow gets an agent, not a refusal. An open still unanswered by then (the prompt
//! left up, or a network volume that hangs) is `matter.folder_permission` too, never
//! `matter.folder_missing` and never a launch let through to fail in the agent. It runs first
//! because the second check reads inside the folder, which would wait on the same prompt.
//!
//! Then, a folder that is gone answers `matter.folder_missing` here, so the agent is never
//! spawned in a folder that is not there (where the spawn would fail as `agent.launch_failed`,
//! naming the folder). It runs off the actor like the matters' probes (`matters/probe.rs`,
//! #617), waited on for [`PROBE_WAIT`]: a folder that does not answer is `matter.folder_missing`
//! too. The check is `check_matter_folder`'s, the matters crate's reason check; a folder that
//! is there without its marker is let through, since the matter's first open may relink it
//! (decision record 0020) and otherwise answers `matter.folder_not_a_matter` itself. No path
//! goes in the refusal or the log.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::{ErrorCode, MatterId};

use crate::actor::matters::probe::{NO_ANSWER, PROBE_WAIT};
use crate::agent::codex::prepare::{Job, off_actor};
use crate::agent::folder_access::{self, PREFLIGHT_WAIT};
use crate::clock::Clock;
use crate::error::Error;
use crate::log_file::Cause;

/// Whether the host may open matter `matter`'s folder `stored` and it is there, with the host's
/// open ([`folder_access::open_folder`]): [`check_with`].
pub(super) async fn check(
    clock: Arc<dyn Clock>,
    matter: MatterId,
    stored: PathBuf,
) -> Result<(), Error> {
    check_with(clock, matter, stored, folder_access::open_folder).await
}

/// [`check`] with `open` as the host's open of the folder: `matter.folder_permission` when
/// the open is refused permission ([`folder_access::preflight`]) or does not answer within
/// [`PREFLIGHT_WAIT`]; then `matter.folder_missing` when the folder is not there, or when that
/// check does not answer within [`PROBE_WAIT`].
pub(super) async fn check_with(
    clock: Arc<dyn Clock>,
    matter: MatterId,
    stored: PathBuf,
    open: impl FnOnce(&Path) -> io::Result<()> + Send + 'static,
) -> Result<(), Error> {
    let access = Job {
        what: "launch folder access",
        matter: Some(matter),
        wait: PREFLIGHT_WAIT,
    };
    let folder = stored.clone();
    let opened = off_actor(Arc::clone(&clock), access, move || {
        folder_access::preflight(&folder, open)
    })
    .await;
    match opened {
        Ok(()) => {}
        Err(error) if error.code() == ErrorCode::MatterFolderPermission => return Err(error),
        // The wait's own failures (no answer in time, no thread): the prompt may still be up.
        Err(error) => {
            tracing::warn!(error = %Cause(&error), matter = %matter, "a launch's matter folder did not open in time; the launch is refused as the privacy settings' refusal");
            return Err(folder_access::refusal());
        }
    }
    let job = Job {
        what: "launch folder check",
        matter: Some(matter),
        wait: PROBE_WAIT,
    };
    let found = off_actor(clock, job, move || {
        match crate::check_matter_folder(&stored, matter) {
            Err(refusal) if refusal.code() == ErrorCode::MatterFolderMissing => {
                Err(Error::Refused(refusal))
            }
            _ => Ok(()),
        }
    })
    .await;
    match found {
        Ok(()) => Ok(()),
        // The check's own refusal; the others are the wait's (no answer, no thread).
        Err(error) if error.code() == ErrorCode::MatterFolderMissing => Err(error),
        Err(error) => {
            tracing::warn!(error = %Cause(&error), matter = %matter, "a launch's matter folder did not answer");
            Err(Error::refused(ErrorCode::MatterFolderMissing, NO_ANSWER))
        }
    }
}

/// The matter folder gone between the launch's folder check (`launch_checks/folder.rs`) and the
/// spec's check of its working folder: `matter.folder_missing`, as the check would have said,
/// with no path. Any other spec failure is passed on.
pub(in crate::actor::turn) fn gone_at_spawn(error: Error) -> Error {
    if matches!(
        error,
        Error::Process(clauseo_process::Error::NoWorkingFolder)
    ) {
        return Error::refused(
            ErrorCode::MatterFolderMissing,
            "the matter's folder is not there",
        );
    }
    error
}

#[cfg(test)]
mod tests;
