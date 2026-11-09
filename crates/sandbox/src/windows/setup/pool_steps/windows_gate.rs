//! The real [`Gate`](super::Gate)'s reads, which each pool task's machine calls: this process's
//! token, its exe's final path, the admin copy's path under `FOLDERID_ProgramFiles`, the record in
//! `HKLM\SOFTWARE\Clauseo\Sandbox` (64-bit view), and the pool-user check.

use std::path::PathBuf;

use super::{Failed, Pool};
use crate::windows::admin_copy::{self, os};
use crate::windows::setup::machine::MachineError;
use crate::windows::setup::owner::this_process;
use crate::windows::setup::processes;
use crate::windows::setup::record_write::{self, RECORD_KEY};

/// Whether this process's token user is LOCAL SYSTEM.
pub(crate) fn is_system() -> Result<bool, Failed> {
    this_process()
        .map(|(_elevated, system)| system)
        .map_err(|error| error.to_string())
}

/// This process's exe, its final path.
pub(crate) fn current_exe() -> Result<PathBuf, Failed> {
    os::running_image().map_err(|error| error.to_string())
}

/// The admin copy's path.
pub(crate) fn admin_copy() -> Result<PathBuf, Failed> {
    let program_files = os::program_files().map_err(|error| error.to_string())?;
    Ok(admin_copy::admin_exe(&program_files))
}

/// The record's `OwnerSid` and `PoolSids`; `None` when there is no record.
pub(crate) fn pool() -> Result<Option<Pool>, Failed> {
    let record = record_write::read(RECORD_KEY).map_err(|error| error.to_string())?;
    Ok(record.map(|record| Pool {
        owner: record.owner,
        users: record.pool,
    }))
}

/// Whether every SID of `pool.users` passes the pool-user check.
pub(crate) fn check_pool(pool: &Pool) -> Result<(), Failed> {
    // The check's own refusal names the SID; the Application log is readable by every local
    // user, so the line says only that one failed.
    processes::check_pool(&pool.users, &pool.owner).map_err(|error| match error {
        MachineError::Other(_) => "a SID PoolSids lists failed the pool-user check".to_owned(),
        error @ (MachineError::Os { .. }
        | MachineError::LockHeld
        | MachineError::Unsupported(_)
        | MachineError::ProfileChanged(_)
        | MachineError::RecordCorrupt(_)) => error.to_string(),
    })
}
