//! `--adopt` (S15 tier B, "Admin copy" in 0049): the installer's elevated step extracts the
//! setup exe into `%ProgramFiles%\Clauseo\Sandbox\` and runs that copy with this mode. In order,
//! each step refusing before anything later is touched:
//!
//! 1. the token: elevated (per user), or elevated or SYSTEM (`--per-machine`), else
//!    [`SetupExit::NotElevated`];
//! 2. the running exe is the admin copy: `%ProgramFiles%`, `Clauseo` and `Sandbox` are folders
//!    and the copy a file, none a link or other reparse point, and the final path of the running
//!    image (`GetFinalPathNameByHandleW`) is `admin_copy::admin_exe`'s, else
//!    [`SetupExit::NotAdminCopy`]. Only the admin copy's place is checked, never the app's;
//! 3. per user, the lawyer: `--owner-sid`, as the unelevated installer named it, must be that
//!    process's token user, found one hop past D-B2's parent (`owner/installer.rs`), else
//!    `Failed`; the elevated account's own user and profile are never used;
//! 3. the set-up lock (`setup::lock`, the named mutex the set-up, `--remove` and the refresh
//!    take), tried without waiting, else [`SetupExit::LockHeld`]; held from here through step 7,
//!    so a refresh never swaps the copy between its hash and the record's write, and given back
//!    before step 8, whose set-up takes it again;
//! 3. one owner, of one kind (D5): per user, a pool owned by another SID (`OwnerSid`) or an
//!    admin copy another install placed (`AdminCopyOwner`: another SID, or `machine`); per
//!    machine, an admin copy a per-user install placed. Either is [`SetupExit::OtherOwner`] with
//!    [`Blocked::OtherAccount`] as its cause, nothing changed. A recorded `AdminCopyVersion`
//!    newer than this build's is [`SetupExit::Downgrade`] (`admin_copy::version_not_older`);
//! 4. the folders: per user, the install folder (an existing folder, no link) and the data folder
//!    lie strictly inside that SID's profile (`ProfileList`); `--per-machine`, the install
//!    folder is any existing folder, no link, that only SYSTEM, Administrators or
//!    `TrustedInstaller` can change (`admin_copy::level_safe`). Else `Usage`;
//! 5. the admin folder holds only the copy, its signature and `staging\` (anything else is
//!    deleted), and it and everything in it get the locked owner and DACL, read back exactly
//!    (`admin_copy::locked`), else `Failed`;
//! 6. per user, the SYSTEM tasks (`refresh::register_task`, PR 3) are registered for the
//!    lawyer, in `refresh::TASKS`' order: `\Clauseo\SandboxRefresh`, `\Clauseo\SandboxStop`,
//!    then `\Clauseo\SandboxSweep`. Any failing is `Failed` with no value written, so the
//!    launcher's check finds the copy missing rather than running it with no task to refresh
//!    it, to stop the pool's processes or to sweep its tasks (a refresh before the values exist
//!    exits 33, changing nothing). A per-machine install doesn't update itself and has no owner
//!    yet, so it gets none here; its set-up registers the stop and sweep tasks for the owner it
//!    records (`late_steps::stop_task`);
//! 7. the copy's SHA-256, read from its file in the admin folder (a reparse point refused), the
//!    install folder, this build's version and the owner (the SID, or `machine`) are written as
//!    the record's `AdminCopySha256`, `UserInstallDir`, `AdminCopyVersion` and
//!    `AdminCopyOwner`. `UserInstallDir` is checked as text only, and stays untrusted (0049);
//! 8. per user, the normal `--set-up` for the lawyer and the data folder runs in this process
//!    (which also requires the folder to be the lawyer's own), and its exit is `--adopt`'s;
//!    `--per-machine` sets nobody up (each user's set-up runs later through the app) and exits
//!    `Done`.
//!
//! Every step goes through [`Adopter`], so the order and the refusals are tested on every OS;
//! `windows_adopter.rs` is the machine.

use std::path::{Path, PathBuf};

use super::explain::{AdoptFailure, AdoptStep, Cause};
use super::outcome::SetupExit;
use super::record_values::{AdminValues, CopyOwner};
use crate::Blocked;
use crate::windows::admin_copy::{self, Found, inside, level_safe, same_path};
use crate::windows::refresh::{SystemTask, TASKS};
use crate::windows::sids::Sid;

#[cfg(test)]
mod tests;
#[cfg(windows)]
pub(crate) mod windows_adopter;

/// Who `--adopt` places the copy for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A per-user install, for the lawyer the unelevated installer named.
    User {
        /// `--owner-sid`.
        owner_sid: Sid,
        /// `--data-dir`.
        data_dir: PathBuf,
    },
    /// A per-machine install (`--per-machine`): no set-up.
    Machine,
}

/// What `--adopt` reads and changes. Errors are causes for the log and the result file, never
/// shown to anyone.
pub trait Adopter {
    /// `%ProgramFiles%`, from `FOLDERID_ProgramFiles`.
    fn program_files(&mut self) -> Result<PathBuf, Cause>;
    /// The final path of the running image, every link on the way resolved.
    fn running_image(&mut self) -> Result<PathBuf, Cause>;
    /// This process's token: elevated, and LOCAL SYSTEM.
    fn token(&mut self) -> Result<(bool, bool), Cause>;
    /// The lawyer, once the unelevated installer's token user is `claimed` (`Owner::of_installer`).
    fn installing_user(&mut self, claimed: &Sid) -> Result<Sid, Cause>;
    /// The record's pool owner (`OwnerSid`), admin copy owner (`AdminCopyOwner`) and version
    /// (`AdminCopyVersion`), each `None` when missing.
    fn records(&mut self) -> Result<Records, Cause>;
    /// `sid`'s profile folder (`ProfileList`), when it has one that isn't temporary.
    fn profile_of(&mut self, sid: &Sid) -> Result<Option<PathBuf>, Cause>;
    /// What is at `path`, never following a link.
    fn found(&mut self, path: &Path) -> Found;
    /// `path`'s owner and DACL, self-relative, read from the name itself.
    fn security(&mut self, path: &Path) -> Result<Vec<u8>, Cause>;
    /// Deletes what the folder `dir` mustn't hold, then gives it, the copy `exe` and the rest
    /// of what is in it their locked owner and DACL, read back.
    fn lock_down(&mut self, dir: &Path, exe: &Path) -> Result<(), Cause>;
    /// The SHA-256 of `exe`, 64 lowercase hex digits; a reparse point refused.
    fn sha256(&mut self, exe: &Path) -> Result<String, Cause>;
    /// Tries the set-up lock without waiting: `Ok(false)` while another holds it.
    fn lock(&mut self) -> Result<bool, Cause>;
    /// Gives the set-up lock back.
    fn unlock(&mut self);
    /// Writes the record's admin-copy values.
    fn write_values(&mut self, values: &AdminValues) -> Result<(), Cause>;
    /// Registers (or updates) the SYSTEM task `task` for the record owner `owner`
    /// (`refresh::register_task`, S15 tier B PR 3).
    fn register_task(&mut self, task: SystemTask, owner: &Sid) -> Result<(), Cause>;
    /// The normal `--set-up` for the lawyer found and `data_dir`, in this process.
    fn set_up(&mut self, data_dir: &Path) -> SetupExit;
    /// Records a step that failed (`explain.rs`): the Event Log's line, and, once the lawyer is
    /// confirmed, `setup-result.json` written as them.
    fn report(&mut self, failure: &AdoptFailure);
}

/// What [`Adopter::records`] reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Records {
    /// `OwnerSid`: the pool's owner.
    pub pool_owner: Option<Sid>,
    /// `AdminCopyOwner`.
    pub copy_owner: Option<CopyOwner>,
    /// `AdminCopyVersion`.
    pub version: Option<String>,
}

/// Runs `--adopt` as the module says, for `target` with the install folder `install_dir`.
pub fn adopt(machine: &mut dyn Adopter, install_dir: &Path, target: &Target) -> SetupExit {
    let per_machine = *target == Target::Machine;
    match machine.token() {
        Ok((elevated, system)) if elevated || (per_machine && system) => {}
        Ok(_) => {
            tracing::error!("--adopt needs an elevated token");
            return SetupExit::NotElevated;
        }
        Err(error) => return failed(machine, AdoptStep::Token, "this process's token", error),
    }
    let program_files = match machine.program_files() {
        Ok(path) => path,
        Err(error) => return failed(machine, AdoptStep::ProgramFiles, "Program Files", error),
    };
    let exe = admin_copy::admin_exe(&program_files);
    if let Err(exit) = check_place(machine, &program_files, &exe) {
        return exit;
    }
    let owner = match target {
        Target::User { owner_sid, .. } => CopyOwner::User(owner_sid.clone()),
        Target::Machine => CopyOwner::Machine,
    };
    if let Target::User { owner_sid, .. } = target
        && let Err(error) = machine.installing_user(owner_sid).and_then(|user| {
            (user == *owner_sid)
                .then_some(())
                .ok_or_else(|| Cause::from("another user was found"))
        })
    {
        return failed(machine, AdoptStep::Lawyer, "the lawyer", error);
    }
    match machine.lock() {
        Ok(true) => {}
        Ok(false) => {
            tracing::error!("--adopt: a set-up, removal or refresh holds the set-up lock");
            return SetupExit::LockHeld;
        }
        Err(error) => return failed(machine, AdoptStep::Lock, "the set-up lock", error),
    }
    let placed = place(machine, install_dir, target, &program_files, &exe, owner);
    machine.unlock();
    if let Err(exit) = placed {
        return exit;
    }
    match target {
        Target::User { data_dir, .. } => machine.set_up(data_dir),
        Target::Machine => SetupExit::Done,
    }
}

/// Steps 3 (the records) to 7, with the set-up lock held.
fn place(
    machine: &mut dyn Adopter,
    install_dir: &Path,
    target: &Target,
    program_files: &Path,
    exe: &Path,
    owner: CopyOwner,
) -> Result<(), SetupExit> {
    check_records(machine, &owner)?;
    let allowed = match target {
        Target::User {
            owner_sid,
            data_dir,
        } => {
            let profile = user_profile(machine, owner_sid)?;
            inside(install_dir, &profile)
                && inside(data_dir, &profile)
                && machine.found(install_dir) == Found::Folder
        }
        Target::Machine => {
            machine.found(install_dir) == Found::Folder
                && machine
                    .security(install_dir)
                    .is_ok_and(|bytes| level_safe(&bytes))
        }
    };
    if !allowed {
        tracing::error!(
            per_machine = *target == Target::Machine,
            "--adopt's folders aren't the lawyer's (or an administrators-only install folder)"
        );
        return Err(SetupExit::Usage);
    }
    let dir = admin_copy::admin_dir(program_files);
    if let Err(error) = machine.lock_down(&dir, exe) {
        return Err(failed(
            machine,
            AdoptStep::LockDown,
            "the admin folder",
            error,
        ));
    }
    let sha256 = match machine.sha256(exe) {
        Ok(sha256) if admin_copy::is_sha256_hex(&sha256) => sha256,
        Ok(_) => {
            let error = Cause::from("not a SHA-256");
            return Err(failed(
                machine,
                AdoptStep::Hash,
                "the admin copy's hash",
                error,
            ));
        }
        Err(error) => return Err(failed(machine, AdoptStep::Hash, "the admin copy", error)),
    };
    if let Target::User { owner_sid, .. } = target {
        for task in TASKS {
            if let Err(error) = machine.register_task(task, owner_sid) {
                return Err(failed(machine, AdoptStep::Task(task), task.name(), error));
            }
        }
    }
    let values = AdminValues {
        sha256,
        install_dir: install_dir.to_path_buf(),
        version: admin_copy::APP_VERSION.to_owned(),
        owner,
    };
    if let Err(error) = machine.write_values(&values) {
        return Err(failed(
            machine,
            AdoptStep::Values,
            "the admin values",
            error,
        ));
    }
    Ok(())
}

/// Logs `error` at `step` (`what` failed), reports it ([`Adopter::report`]), and gives
/// [`SetupExit::Failed`].
fn failed(machine: &mut dyn Adopter, step: AdoptStep, what: &str, error: Cause) -> SetupExit {
    tracing::error!(error = %error.text, step = step.name(), "--adopt: {what} failed");
    machine.report(&AdoptFailure { step, cause: error });
    SetupExit::Failed
}

/// Step 2: the running image is the admin copy at `exe`, through no link.
fn check_place(
    machine: &mut dyn Adopter,
    program_files: &Path,
    exe: &Path,
) -> Result<(), SetupExit> {
    let mut at = program_files.to_path_buf();
    let mut levels = vec![(at.clone(), Found::Folder)];
    for part in admin_copy::ADMIN_DIR {
        at = at.join(part);
        levels.push((at.clone(), Found::Folder));
    }
    levels.push((exe.to_path_buf(), Found::File));
    let plain = levels
        .iter()
        .all(|(path, want)| machine.found(path) == *want);
    match machine.running_image() {
        Ok(running) if plain && same_path(&running, exe) => Ok(()),
        Ok(_) => {
            tracing::error!(
                "--adopt runs only as the admin copy, through no link; nothing changed"
            );
            Err(SetupExit::NotAdminCopy)
        }
        Err(error) => Err(failed(
            machine,
            AdoptStep::RunningImage,
            "the image's path",
            error,
        )),
    }
}

/// Step 3: one owner, of one kind, and no downgrade.
fn check_records(machine: &mut dyn Adopter, owner: &CopyOwner) -> Result<(), SetupExit> {
    let records = match machine.records() {
        Ok(records) => records,
        Err(error) => return Err(failed(machine, AdoptStep::Record, "the record", error)),
    };
    let other_pool = match owner {
        CopyOwner::User(sid) => records.pool_owner.as_ref().is_some_and(|pool| pool != sid),
        CopyOwner::Machine => false,
    };
    let other_copy = records
        .copy_owner
        .as_ref()
        .is_some_and(|placed| placed != owner);
    if other_pool || other_copy {
        tracing::error!(cause = %Blocked::OtherAccount, "--adopt changed nothing");
        return Err(SetupExit::OtherOwner);
    }
    if let Some(recorded) = &records.version
        && !admin_copy::version_not_older(recorded, admin_copy::APP_VERSION)
    {
        tracing::error!("--adopt won't replace a newer admin copy; nothing changed");
        return Err(SetupExit::Downgrade);
    }
    Ok(())
}

/// The lawyer's profile (`ProfileList`); otherwise the exit.
fn user_profile(machine: &mut dyn Adopter, user: &Sid) -> Result<PathBuf, SetupExit> {
    match machine.profile_of(user) {
        Ok(Some(profile)) => Ok(profile),
        Ok(None) => {
            tracing::error!("--adopt: the lawyer has no profile");
            Err(SetupExit::Usage)
        }
        Err(error) => Err(failed(machine, AdoptStep::Profile, "the profile", error)),
    }
}
