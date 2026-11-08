//! `--remove`'s last step (S15 tier B, "Admin copy" in 0049): once no tier-B owner is left on
//! the machine, the admin copy goes too. In `refresh::TEARDOWN`'s order: the record's four
//! admin-copy values (`ADMIN_VALUES`, so no launcher trusts the copy from here on), then the
//! SYSTEM tasks `\Clauseo\SandboxRefresh` and `\Clauseo\SandboxStop` ([`unregister_task`]),
//! then the refresh's staging ([`remove_staging`], taken back if it shuts Administrators out),
//! then everything in
//! `%ProgramFiles%\Clauseo\Sandbox\`, then the folder. Only the uninstaller's
//! `--remove --uninstall` runs it: the app's in-app removal keeps all of these.
//!
//! The running exe is usually the admin copy itself, which Windows won't delete while it runs.
//! It is renamed in place to a name nothing starts (`clauseo-sandbox-setup.exe.<pid>.delete`)
//! and that name is scheduled for deletion at the next restart (`MoveFileExW` with
//! `MOVEFILE_DELAY_UNTIL_REBOOT`), as are the `Sandbox` folder and `%ProgramFiles%\Clauseo`
//! after it; Windows deletes a scheduled folder only if it is empty by then, so a reinstall
//! before the restart keeps its new copy. If the rename fails, the copy stays where it is and
//! the step fails: the canonical name is never scheduled, since a reinstall before the restart
//! would put its new copy there and lose it at boot. Run from anywhere else (a test, a VM
//! harness), the copy and the folder are deleted at once.
//!
//! The last step also deletes `%ProgramFiles%\Clauseo\installer-temp`, where earlier installers
//! made their plug-in folders, only when empty ([`clear_old_installer_temp`]).
//!
//! A step that fails stops the teardown and is named ([`CopyLeft`]): `exe::run_remove` logs
//! it to the Event Log and exits `AdminCopyLeft`.
#![expect(unsafe_code, reason = "FFI to MoveFileExW")]
#![expect(
    clippy::disallowed_methods,
    reason = "the elevated removal deletes the admin folder, which only administrators can change \
              and which holds no file of the lawyer's: not the data folder's write module's work"
)]

use std::io;
use std::iter::once;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use windows::Win32::Storage::FileSystem::{MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW};
use windows::core::PCWSTR;

use super::explain::{Cause, CopyLeft};
use super::machine::MachineError;
use super::record_write;
use crate::windows::admin_copy::take_back::remove_taking_back;
use crate::windows::admin_copy::take_back_os::NtfsLockDown;
use crate::windows::admin_copy::{self, Found, os, same_path};
use crate::windows::refresh::{self, SystemTask, TEARDOWN, Teardown};

/// The folder in `%ProgramFiles%\Clauseo` where earlier installers made their plug-in folders.
const OLD_INSTALLER_TEMP: &str = "installer-temp";

/// Deletes the admin copy as the module says, step by step in [`TEARDOWN`]'s order. The
/// record's values first: a failure there stops before any task or file is touched. A step
/// that fails is named, with why.
pub(crate) fn remove_admin_copy(record_key: &str) -> Result<(), CopyLeft> {
    for step in TEARDOWN {
        let done = match step {
            Teardown::AdminValues => record_write::delete_admin_values(record_key),
            Teardown::Task(task) => unregister_task(task),
            Teardown::Staging => remove_staging(),
            Teardown::AdminFolder => remove_admin_folder(),
        };
        done.map_err(|error| CopyLeft {
            step: teardown_name(step),
            cause: Cause::from(error),
        })?;
    }
    Ok(())
}

/// A teardown step's name, for the Event Log.
pub(super) fn teardown_name(step: Teardown) -> &'static str {
    match step {
        Teardown::AdminValues => "admin_values",
        Teardown::Task(SystemTask::Refresh) => "refresh_task",
        Teardown::Task(SystemTask::Stop) => "stop_task",
        Teardown::Task(SystemTask::Watch) => "watch_task",
        Teardown::Staging => "staging",
        Teardown::AdminFolder => "admin_folder",
    }
}

/// The last step: everything in the admin folder, then the folder ([`clear`]).
fn remove_admin_folder() -> Result<(), MachineError> {
    let program_files = os::program_files().map_err(|e| other(&e))?;
    let dir = admin_copy::admin_dir(&program_files);
    match os::found(&dir).map_err(|e| other(&e))? {
        Found::Missing => return Ok(()),
        Found::Folder => {}
        Found::File | Found::Link => {
            return Err(MachineError::Other(
                "the admin folder isn't a plain folder; left as it is".to_owned(),
            ));
        }
    }
    let exe = admin_copy::admin_exe(&program_files);
    let running = std::env::current_exe().is_ok_and(|running| same_path(&running, &exe));
    let app = program_files.join(admin_copy::ADMIN_DIR[0]);
    clear_old_installer_temp(&app);
    let mut rename = |from: &Path, to: &Path| std::fs::rename(from, to);
    let mut schedule = delete_at_restart;
    let mut fs = Restart {
        rename: &mut rename,
        schedule: &mut schedule,
    };
    clear(&dir, &exe, running, &app, &mut fs).map_err(|e| other(&e))
}

/// Deletes `app`'s (`%ProgramFiles%\Clauseo`) [`OLD_INSTALLER_TEMP`], earlier installers' plug-in
/// parent, when empty. Never recursive, so an older installer running meanwhile keeps its
/// folder, and a link there goes as itself, never its target; a failure changes nothing.
pub(super) fn clear_old_installer_temp(app: &Path) {
    drop(std::fs::remove_dir(app.join(OLD_INSTALLER_TEMP)));
}

/// How [`clear`] parks the running copy: the rename, and the delete at restart.
pub(super) struct Restart<'a> {
    /// Renames the first path to the second (`std::fs::rename`).
    pub rename: &'a mut dyn FnMut(&Path, &Path) -> io::Result<()>,
    /// Schedules a path for deletion at restart (`MoveFileExW`).
    pub schedule: &'a mut dyn FnMut(&Path) -> io::Result<()>,
}

/// Deletes everything in the admin folder `dir`, then `dir`. When the copy `exe` is the
/// running exe (`running`), it is renamed in place to [`parked_name`] instead, and `schedule`
/// is given that name, `dir` and `app` (`%ProgramFiles%\Clauseo`), in that order, to delete
/// at restart; if the rename fails, nothing is scheduled and the rename's error is returned,
/// `exe` left in place (never scheduled under its own name). A link inside is
/// deleted as itself, never followed; an entry that shuts Administrators out is taken back
/// first (`admin_copy::take_back`).
pub(super) fn clear(
    dir: &Path,
    exe: &Path,
    running: bool,
    app: &Path,
    fs: &mut Restart<'_>,
) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if running && same_path(&path, exe) {
            continue;
        }
        remove_taking_back(&mut NtfsLockDown, &path)?;
    }
    if !running {
        return std::fs::remove_dir(dir);
    }
    let parked = parked_name(exe, std::process::id());
    if let Err(error) = (fs.rename)(exe, &parked) {
        tracing::warn!(%error, "the running copy can't be renamed; it is left in place");
        return Err(error);
    }
    for path in [parked.as_path(), dir, app] {
        (fs.schedule)(path)?;
    }
    Ok(())
}

/// PR 3's hook: deletes the SYSTEM task `task` (`refresh::unregister_task`; one that isn't
/// there is `Ok`). Run by the uninstaller's removal only, before the admin folder goes.
pub(crate) fn unregister_task(task: SystemTask) -> Result<(), MachineError> {
    refresh::unregister_task(task).map_err(|error| MachineError::Other(error.to_string()))
}

/// Deletes the refresh task's staging folder, `%ProgramFiles%\Clauseo\Sandbox\staging\`, with
/// whatever is in it (a refresh's set-aside `.old` copy, a staged `.new`), taking it back first
/// when its DACL shuts Administrators out (`admin_copy::take_back`), as an older refresh's
/// SYSTEM-only one does.
pub(crate) fn remove_staging() -> Result<(), MachineError> {
    let program_files = os::program_files().map_err(|e| other(&e))?;
    let staging = admin_copy::admin_dir(&program_files).join(admin_copy::STAGING);
    remove_taking_back(&mut NtfsLockDown, &staging).map_err(|e| other(&e))
}

/// The name the running copy is renamed to in its own folder: one nothing starts.
pub(super) fn parked_name(exe: &Path, pid: u32) -> PathBuf {
    let mut name = exe.as_os_str().to_owned();
    name.push(format!(".{pid}.delete"));
    PathBuf::from(name)
}

/// Schedules `path` for deletion at the next restart (`MOVEFILE_DELAY_UNTIL_REBOOT` with no
/// new name). Needs an elevated or SYSTEM token, which `--remove` has.
fn delete_at_restart(path: &Path) -> io::Result<()> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call; a null new name asks for a delete.
    unsafe {
        MoveFileExW(
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            MOVEFILE_DELAY_UNTIL_REBOOT,
        )
    }
    .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF))
}

/// An I/O error as a removal error, its Win32 code kept when it has one.
fn other(error: &io::Error) -> MachineError {
    match error
        .raw_os_error()
        .and_then(|code| u32::try_from(code).ok())
    {
        Some(code) => MachineError::Os {
            call: "deleting the admin folder",
            code,
        },
        None => MachineError::Other(error.to_string()),
    }
}

#[cfg(test)]
mod tests;
