//! Windows security blocking the admin copy (`Blocked::ToolMissing`), remembered for this user.
//!
//! When the elevated start of the admin copy fails with `ERROR_VIRUS_INFECTED` (225) or
//! `ERROR_VIRUS_DELETED` (226), Windows security has refused the copy and may have deleted it.
//! The next check would then find no copy and say `AdminCopyMissing`, whose repair is "get the
//! installer", and Windows security would undo the reinstall again. So the start records
//! `SecurityBlockedAt` (a `REG_QWORD` `FILETIME` under `HKCU\Software\Clauseo\Sandbox`,
//! `mark_os`), and while it is there a copy that isn't there as a regular file is
//! `ToolMissing`, in the launch check and the start alike. A start that succeeds, or a copy
//! found and verified (its hash the recorded one), deletes it.
//!
//! The mark is per user and trusted for nothing but which message to show: it can only turn
//! `AdminCopyMissing` into `ToolMissing`, and both refuse. One that can't be read counts as
//! absent, and one that can't be written or deleted is logged and changes nothing else.

use std::path::Path;

use super::{AdminCopyFs, launch_checked};
use crate::{Blocked, SetupError};

#[cfg(test)]
mod tests;

/// The cause for a copy that isn't there as a regular file: [`Blocked::ToolMissing`] while the
/// mark is there, else [`Blocked::AdminCopyMissing`].
pub(super) fn missing(fs: &dyn AdminCopyFs) -> Blocked {
    match fs.security_blocked() {
        Ok(true) => {
            tracing::warn!("the admin copy is gone after Windows security blocked it");
            Blocked::ToolMissing
        }
        Ok(false) => Blocked::AdminCopyMissing,
        Err(error) => {
            tracing::warn!(%error, "the security-block mark can't be read");
            Blocked::AdminCopyMissing
        }
    }
}

/// The copy is there and verified: a mark left from an earlier block is deleted.
pub(super) fn verified(fs: &dyn AdminCopyFs) {
    if matches!(fs.security_blocked(), Ok(true)) {
        clear(fs);
    }
}

/// Deletes the mark, logging a failure.
fn clear(fs: &dyn AdminCopyFs) {
    if let Err(error) = fs.set_security_blocked(false) {
        tracing::warn!(%error, "the security-block mark couldn't be deleted");
    }
}

/// [`launch_checked`] for `UserSetup`'s start: a failed check is [`SetupError::Blocked`] with
/// nothing started; a start that gives [`Blocked::ToolMissing`] (225 or 226, `launch_error`)
/// records the mark, and a start that succeeds deletes it.
pub fn launch_recorded<T>(
    fs: &dyn AdminCopyFs,
    start: impl FnOnce(&Path) -> Result<T, SetupError>,
) -> Result<T, SetupError> {
    let started = launch_checked(fs, start).map_err(SetupError::Blocked)?;
    match &started {
        Ok(_) => clear(fs),
        Err(SetupError::Blocked(Blocked::ToolMissing)) => {
            if let Err(error) = fs.set_security_blocked(true) {
                tracing::warn!(%error, "the security block couldn't be recorded");
            }
        }
        Err(_) => {}
    }
    started
}
