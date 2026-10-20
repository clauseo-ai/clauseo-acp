//! The staging dirs `setup --binaries` downloads into, inside cargo's bin, and the run lock that
//! lets the sweep at the start of a run remove every one an interrupted run left.

use super::{Failure, retry_fix};
use crate::output::Printer;
use crate::setup::Machine;
use crate::write;
use std::fs::{File, TryLockError};
use std::io::{self, Write};
use std::path::Path;

/// The start of the name of every staging directory `setup` makes in bin.
pub(super) const STAGING_PREFIX: &str = ".xtask-setup-";

/// Makes `bin` if needed, and `staging` in it, empty.
pub(super) fn fresh_dir(bin: &Path, staging: &Path) -> Result<(), Failure> {
    write::reset_staging_dir(bin, staging).map_err(|error| {
        (
            format!("could not make {}: {error}", staging.display()),
            retry_fix(bin),
        )
    })
}

/// The file in bin whose exclusive lock `setup --binaries` holds for the whole run, so one run at a
/// time uses bin. It is never deleted, and `STAGING_PREFIX` does not match it.
const RUN_LOCK: &str = ".xtask-setup.lock";

/// On macOS and Linux, takes the run lock in `machine`'s cargo bin, waiting while another
/// `setup --binaries` holds it, then removes every `STAGING_PREFIX` entry. With the lock held no
/// other run uses one, so each is from a run that was interrupted (Ctrl-C, a kill). When the lock
/// cannot be taken it notes why and goes on without it and without the sweep. Returns the lock,
/// held until dropped, and whether the sweep worked.
pub(crate) fn lock_bin_and_sweep<W: Write>(
    out: &mut Printer<W>,
    machine: &Machine<'_>,
) -> (Option<File>, bool) {
    let Some(bin) = machine
        .cargo_bin
        .as_deref()
        .filter(|_| machine.os != "windows")
    else {
        return (None, true);
    };
    let lock = match lock_run(out, bin) {
        Ok(lock) => lock,
        Err(error) => {
            out.note(format!(
                "could not lock {} ({error}), so this run does not remove what an interrupted \
                 setup left in {}",
                bin.join(RUN_LOCK).display(),
                bin.display()
            ));
            return (None, true);
        }
    };
    let swept = write::remove_staging_leftovers(bin, STAGING_PREFIX);
    if let Err(error) = &swept {
        out.failure(
            &format!(
                "could not remove what an interrupted setup left in {}: {error}",
                bin.display()
            ),
            &format!(
                "remove the {STAGING_PREFIX}* entries in {} by hand",
                bin.display()
            ),
        );
    }
    (Some(lock), swept.is_ok())
}

/// Makes `bin` if needed, then takes the exclusive lock on `RUN_LOCK` in it, waiting while another
/// run holds it. The lock file is opened for reading, so one the user cannot write still locks. It
/// is created only when it is missing.
fn lock_run<W: Write>(out: &mut Printer<W>, bin: &Path) -> io::Result<File> {
    let lock = write::open_run_lock(bin, RUN_LOCK)?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            out.note(format!(
                "another cargo xtask setup --binaries is using {}: waiting for it to finish",
                bin.display()
            ));
            lock.lock()?;
        }
        Err(TryLockError::Error(error)) => return Err(error),
    }
    Ok(lock)
}
