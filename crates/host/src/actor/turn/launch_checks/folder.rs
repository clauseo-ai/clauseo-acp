//! The launch's first check, before its preparation makes anything: that the matter folder is
//! there. A folder that is gone answers `matter.folder_missing` here, so the agent is never
//! spawned in a folder that is not there (where the spawn would fail as `agent.launch_failed`,
//! naming the folder). It runs off the actor like the matters' probes (`matters/probe.rs`,
//! #617), waited on for [`PROBE_WAIT`]: a folder that does not answer is `matter.folder_missing`
//! too. The check is `check_matter_folder`'s, the matters crate's reason check; a folder that
//! is there without its marker is let through, since the matter's first open may relink it
//! (decision record 0020) and otherwise answers `matter.folder_not_a_matter` itself. No path
//! goes in the refusal or the log.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{ErrorCode, MatterId};

use crate::actor::matters::probe::{NO_ANSWER, PROBE_WAIT};
use crate::agent::codex::prepare::{Job, off_actor};
use crate::clock::Clock;
use crate::error::Error;
use crate::log_file::Cause;

/// Whether matter `matter`'s folder `stored` is there: `matter.folder_missing` when it is not,
/// or when the check does not answer within [`PROBE_WAIT`].
pub(super) async fn check(
    clock: Arc<dyn Clock>,
    matter: MatterId,
    stored: PathBuf,
) -> Result<(), Error> {
    let job = Job {
        what: "launch folder check",
        matter: Some(matter),
        wait: PROBE_WAIT,
    };
    let found = off_actor(clock, job, move || {
        Ok(crate::check_matter_folder(&stored, matter))
    })
    .await;
    match found {
        Ok(Err(refusal)) if refusal.code() == ErrorCode::MatterFolderMissing => {
            Err(Error::Refused(refusal))
        }
        Ok(_) => Ok(()),
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
