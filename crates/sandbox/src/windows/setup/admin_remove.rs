//! `--remove`'s last step (S15 tier B, "Admin copy" in 0049): once no tier-B owner is left on
//! the machine, the admin copy goes too. In order: the record's `AdminCopySha256` and
//! `UserInstallDir` (so no launcher trusts the copy from here on), then the refresh task and
//! its staging ([`remove_refresh_task`], S15 tier B PR 3's hook), then everything in
//! `%ProgramFiles%\Clauseo\Sandbox\`, then the folder.
//!
//! The running exe is usually the admin copy itself, which Windows won't delete while it runs.
//! It is renamed in place to a name nothing starts (`clauseo-sandbox-setup.exe.<pid>.delete`)
//! and that name is scheduled for deletion at the next restart (`MoveFileExW` with
//! `MOVEFILE_DELAY_UNTIL_REBOOT`), as are the `Sandbox` folder and `%ProgramFiles%\Clauseo`
//! after it; Windows deletes a scheduled folder only if it is empty by then, so a reinstall
//! before the restart keeps its new copy. Run from anywhere else (a test, a VM harness), the
//! copy and the folder are deleted at once.
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

use super::machine::MachineError;
use super::record_write;
use crate::windows::admin_copy::{self, Found, os, same_path};

/// Deletes the admin copy as the module says. The record's values first: a failure there
/// stops before any file is touched.
pub(crate) fn remove_admin_copy(record_key: &str) -> Result<(), MachineError> {
    record_write::delete_admin_values(record_key)?;
    remove_refresh_task()?;
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
    let running = std::env::current_exe().ok();
    let exe = admin_copy::admin_exe(&program_files);
    let is_running = running
        .as_deref()
        .is_some_and(|running| same_path(running, &exe));
    for entry in std::fs::read_dir(&dir).map_err(|e| other(&e))? {
        let path = entry.map_err(|e| other(&e))?.path();
        if is_running && same_path(&path, &exe) {
            continue;
        }
        // `remove_dir_all` deletes a link inside, never what it points at.
        match os::found(&path).map_err(|e| other(&e))? {
            Found::Folder => std::fs::remove_dir_all(&path),
            Found::File | Found::Link => std::fs::remove_file(&path),
            Found::Missing => Ok(()),
        }
        .map_err(|e| other(&e))?;
    }
    if !is_running {
        return std::fs::remove_dir(&dir).map_err(|e| other(&e));
    }
    let parked = parked_name(&exe, std::process::id());
    std::fs::rename(&exe, &parked).map_err(|e| other(&e))?;
    for path in [
        parked.as_path(),
        dir.as_path(),
        &program_files.join(admin_copy::ADMIN_DIR[0]),
    ] {
        delete_at_restart(path).map_err(|e| other(&e))?;
    }
    Ok(())
}

/// The hook S15 tier B PR 3 fills: deletes the SYSTEM refresh task `\Clauseo\SandboxRefresh`
/// and its staging folder, `%ProgramFiles%\Clauseo\Sandbox\staging\`, before the admin folder
/// goes. Nothing yet: this PR makes neither (the staging folder, if any, goes with the rest of
/// the admin folder's contents).
#[expect(
    clippy::unnecessary_wraps,
    reason = "PR 3 fills it with calls that can fail; the caller already stops on its error"
)]
pub(crate) fn remove_refresh_task() -> Result<(), MachineError> {
    Ok(())
}

/// The name the running copy is renamed to in its own folder: one nothing starts.
fn parked_name(exe: &Path, pid: u32) -> PathBuf {
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

fn other(error: &io::Error) -> MachineError {
    MachineError::Other(error.to_string())
}
