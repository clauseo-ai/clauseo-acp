//! `--adopt` (S15 tier B, "Admin copy" in 0049): the installer's elevated step extracts the
//! setup exe into `%ProgramFiles%\Clauseo\Sandbox\` and runs that copy with this mode. In order,
//! each step refusing before anything later is touched:
//!
//! 1. the token: elevated (per user), or elevated or SYSTEM (`--per-machine`), else
//!    [`SetupExit::NotElevated`];
//! 2. the running exe is the admin copy itself (`current_exe()` against
//!    `admin_copy::admin_exe`), else [`SetupExit::NotAdminCopy`];
//! 3. per user, the installing user is the recorded parent's token user (D-B2, as `--remove`
//!    takes it), and a record owned by another SID is [`SetupExit::OtherOwner`] with
//!    [`Blocked::OtherAccount`] as its cause, nothing changed (D5's one owner);
//! 4. the install folder: per user, an existing folder (no link) strictly inside the installing
//!    user's profile; `--per-machine`, `%ProgramFiles%\Clauseo` itself. Else `Usage`;
//! 5. the admin folder and the copy get their locked owner and DACL, read back exactly
//!    (`admin_copy::locked`), else `Failed`;
//! 6. the copy's SHA-256, read from its file in the admin folder (a reparse point refused), and
//!    the install folder are written as the record's `AdminCopySha256` and `UserInstallDir`;
//! 7. per user, the normal `--set-up` for that user and data folder runs in this process, and
//!    its exit is `--adopt`'s; `--per-machine` sets nobody up (each user's set-up runs later
//!    through the app) and exits `Done`.
//!
//! Every step goes through [`Adopter`], so the order and the refusals are tested on every OS;
//! `windows_adopter.rs` is the machine.

use std::path::{Path, PathBuf};

use super::outcome::SetupExit;
use crate::Blocked;
use crate::windows::admin_copy::{self, inside, same_path};
use crate::windows::sids::Sid;

#[cfg(test)]
mod tests;
#[cfg(windows)]
pub(crate) mod windows_adopter;

/// Who `--adopt` places the copy for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A per-user install: the installing user's data folder, for the set-up that follows.
    User {
        /// `--data-dir`.
        data_dir: PathBuf,
    },
    /// A per-machine install (`--per-machine`): no set-up.
    Machine,
}

/// What `--adopt` reads and changes. Errors are causes for the log, never shown to anyone.
pub trait Adopter {
    /// `%ProgramFiles%`, from `FOLDERID_ProgramFiles`.
    fn program_files(&mut self) -> Result<PathBuf, String>;
    /// The running exe's path.
    fn current_exe(&mut self) -> Result<PathBuf, String>;
    /// This process's token: elevated, and LOCAL SYSTEM.
    fn token(&mut self) -> Result<(bool, bool), String>;
    /// The installing user: the recorded parent's token user.
    fn installing_user(&mut self) -> Result<Sid, String>;
    /// The machine record's owner, when there is a record.
    fn record_owner(&mut self) -> Result<Option<Sid>, String>;
    /// `sid`'s profile folder, when it has one.
    fn profile_of(&mut self, sid: &Sid) -> Result<Option<PathBuf>, String>;
    /// Whether `path` is a folder, not a link or other reparse point.
    fn is_folder(&mut self, path: &Path) -> bool;
    /// Gives the folder `dir` and the copy `exe` their locked owner and DACL, read back.
    fn lock_down(&mut self, dir: &Path, exe: &Path) -> Result<(), String>;
    /// The SHA-256 of `exe`, 64 lowercase hex digits; a reparse point refused.
    fn sha256(&mut self, exe: &Path) -> Result<String, String>;
    /// Writes `AdminCopySha256` and `UserInstallDir`.
    fn write_values(&mut self, sha256: &str, install_dir: &Path) -> Result<(), String>;
    /// The normal `--set-up` for the installing user and `data_dir`, in this process.
    fn set_up(&mut self, data_dir: &Path) -> SetupExit;
}

/// Runs `--adopt` as the module says, for `target` with the install folder `install_dir`.
pub fn adopt(machine: &mut dyn Adopter, install_dir: &Path, target: &Target) -> SetupExit {
    let failed = |what: &str, error: String| {
        tracing::error!(%error, "--adopt: {what}");
        SetupExit::Failed
    };
    let per_machine = *target == Target::Machine;
    match machine.token() {
        Ok((elevated, system)) if elevated || (per_machine && system) => {}
        Ok(_) => {
            tracing::error!("--adopt needs an elevated token");
            return SetupExit::NotElevated;
        }
        Err(error) => return failed("this process's token can't be read", error),
    }
    let program_files = match machine.program_files() {
        Ok(path) => path,
        Err(error) => return failed("Windows gave no Program Files folder", error),
    };
    let exe = admin_copy::admin_exe(&program_files);
    match machine.current_exe() {
        Ok(running) if same_path(&running, &exe) => {}
        Ok(_) => {
            tracing::error!("--adopt runs only from the admin copy; nothing was changed");
            return SetupExit::NotAdminCopy;
        }
        Err(error) => return failed("the running exe's path is unknown", error),
    }
    let within = match target {
        Target::User { .. } => match user_profile(machine) {
            Ok(profile) => Some(profile),
            Err(exit) => return exit,
        },
        Target::Machine => None,
    };
    let allowed = match &within {
        Some(profile) => inside(install_dir, profile),
        None => same_path(install_dir, &program_files.join(admin_copy::ADMIN_DIR[0])),
    };
    // The folder is a full drive path already (`args::path_of`); here it must be the right one.
    if !allowed || !machine.is_folder(install_dir) {
        tracing::error!(
            per_machine,
            "--adopt's install folder isn't the installing user's (or Program Files' Clauseo)"
        );
        return SetupExit::Usage;
    }
    let dir = admin_copy::admin_dir(&program_files);
    if let Err(error) = machine.lock_down(&dir, &exe) {
        return failed("the admin folder couldn't be locked down", error);
    }
    let sha256 = match machine.sha256(&exe) {
        Ok(sha256) if admin_copy::is_sha256_hex(&sha256) => sha256,
        Ok(_) => return failed("the admin copy's hash", "not a SHA-256".to_owned()),
        Err(error) => return failed("the admin copy can't be hashed", error),
    };
    if let Err(error) = machine.write_values(&sha256, install_dir) {
        return failed("the record's admin-copy values couldn't be written", error);
    }
    match target {
        Target::User { data_dir } => machine.set_up(data_dir),
        Target::Machine => SetupExit::Done,
    }
}

/// The installing user's profile, once the record is theirs or nobody's; otherwise the exit.
fn user_profile(machine: &mut dyn Adopter) -> Result<PathBuf, SetupExit> {
    let failed = |what: &str, error: String| {
        tracing::error!(%error, "--adopt: {what}");
        SetupExit::Failed
    };
    let user = machine
        .installing_user()
        .map_err(|error| failed("the installing user can't be found", error))?;
    match machine.record_owner() {
        Ok(Some(owner)) if owner != user => {
            tracing::error!(cause = %Blocked::OtherAccount, "--adopt changed nothing");
            return Err(SetupExit::OtherOwner);
        }
        Ok(_) => {}
        Err(error) => return Err(failed("the machine's record can't be read", error)),
    }
    match machine.profile_of(&user) {
        Ok(Some(profile)) => Ok(profile),
        Ok(None) => {
            tracing::error!("--adopt: the installing user has no profile");
            Err(SetupExit::Usage)
        }
        Err(error) => Err(failed("the installing user's profile can't be read", error)),
    }
}
