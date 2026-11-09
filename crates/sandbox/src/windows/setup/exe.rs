//! The setup exe's work (S15 `PR4h`), which `src/bin/clauseo-sandbox-setup.rs` runs elevated:
//! the command line (`args.rs`), then the mode.
//!
//! `--set-up` takes the owner from the verified parent (`Owner::of_parent`, D-B2), and resolves
//! `--data-dir` as the owner: no junction or link below the drive's root, a folder the owner
//! owns (or Administrators does, for an owner in Administrators), recorded and used as its
//! final path from then on (a `subst` drive resolves to the real folder). One owned by anyone
//! else exits `DataDirNotOwned`, anything else `Usage`, with nothing touched. It builds
//! the request for that owner and checks it names that owner ([`owner_failure`]), then runs
//! [`set_up`] over a [`WindowsMachine`], which reads the registered matters from the owner's
//! files once it holds the lock, and writes `setup-result.json` as the owner. Every read and
//! write under `<data_dir>\sandbox\` runs as the owner (`Machine::as_owner`). An owner that
//! can't be found writes no result (there is no one to write it as); the app then names the
//! exit code. Before anything, the DLL search is kept to System32.
//!
//! `--remove [--per-machine]` (`PR4j`, `remove.rs`) takes the user who asked from the recorded
//! parent too (`Owner::of_parent`, the hook passing no `--parent`); one that can't be found is
//! no one, which a per-user removal refuses and a per-machine one doesn't need. `--per-machine`
//! from a process that is neither elevated nor LOCAL SYSTEM exits `Usage`, nothing touched. It
//! writes no result file: the hook reads only the exit code.
//!
//! `--reclaim` takes its owner from the recorded parent too and its data folder from the
//! machine's record, resolved the same way (`reclaim_run.rs`, S15 `PR4i`).
//!
//! `--adopt` (`adopt.rs`) runs only from the admin copy, which the installer just placed:
//! it locks the admin folder, records the copy's hash and the install folder, and, per user,
//! runs `--set-up` in this process for the lawyer the unelevated installer named and its token
//! confirms (`Owner::of_installer`).
//!
//! After a `--remove --uninstall` that leaves no pool owner on the machine, the admin copy is
//! removed last (`admin_remove.rs`), by the removal of the install that placed it only (the
//! app's plain `--remove` keeps it, with its record values); a failure there is
//! written to the Event Log with its step and code, and exits `AdminCopyLeft` (10), which the
//! uninstall hook counts as removed, as what is left is only administrators' to change.
//!
//! The exe has no log output of its own: `--adopt`'s failures go to the Event Log and the
//! result file (`explain.rs`), and `tracing` lines at warn and above go to the Event Log as
//! their message alone (`event_trace.rs`).
//!
//! `--refresh` (`crate::windows::refresh`) is the SYSTEM task's: it exits with its own codes
//! (`RefreshExit`, from 32), which is why [`main`] returns the process's exit code. So do
//! `--stop` (`stop.rs`, `StopExit`, from 45), `--wsl-deny` (`refresh::WslDenyExit`, 51 and
//! 52), `--sweep-tasks` (`sweep.rs`, `SweepExit`, from 53, after the WSL deny task's 51 and 52)
//! and `--wsl-upgrade` (`refresh::WslUpgradeExit`, from 60, after the sweep's 59), the other
//! SYSTEM tasks'.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::admin_remove::remove_admin_copy;
use super::adopt::Target;
use super::adopt::windows_adopter::run_adopt;
use super::args::{self, Mode};
use super::around::Around;
use super::explain::{AdoptFailure, AdoptStep, Cause, CopyLeft, removal_exit};
use super::ffi::restrict_dll_search;
use super::machine::{MachineError, owned};
use super::outcome::{SetupExit, SetupFailure, SetupReport, Step};
use super::owner::{Owner, this_process};
use super::reclaim_run::run_reclaim;
use super::record_write::RECORD_KEY;
use super::remove::{Removed, WindowsRemoval, per_machine_allowed, remove_then};
use super::result_file::{self, SetupResult};
use super::stop;
use super::sweep;
use super::windows_machine::{Names, WindowsMachine};
use super::{SetupRequest, owner_failure, set_up};
use crate::windows::NtfsAcl;
use crate::windows::admin_copy::{Removal, copy_removable};
use crate::windows::private_dir::resolve_folder;
use crate::windows::refresh;
use crate::windows::sids::{Sid, data_dir_owner_allowed};

/// Runs the exe on `args` (those after its name) and returns its process exit code: a
/// [`SetupExit`]'s, for `--refresh` a [`refresh::RefreshExit`]'s, for `--stop` a
/// [`stop::StopExit`]'s, for `--sweep-tasks` a [`sweep::SweepExit`]'s, for `--wsl-deny` a
/// [`refresh::WslDenyExit`]'s, for `--wsl-upgrade` a [`refresh::WslUpgradeExit`]'s.
pub fn main(args: impl IntoIterator<Item = OsString>) -> u8 {
    // First: no DLL from beside the exe (a folder a standard user may write) loads from here on.
    if let Err(error) = restrict_dll_search() {
        tracing::error!(%error, "the DLL search couldn't be kept to System32");
        let failure = AdoptFailure {
            step: AdoptStep::DllSearch,
            cause: Cause::from(error),
        };
        refresh::event_log(true, &failure.line());
        return SetupExit::Failed.as_code();
    }
    let mode = match args::parse(args) {
        Ok(mode) => mode,
        Err(usage) => {
            tracing::error!(%usage, "the set-up was started wrongly");
            return SetupExit::Usage.as_code();
        }
    };
    if mode == Mode::Refresh {
        return refresh::run().as_code();
    }
    if mode == Mode::Stop {
        return stop::windows_stopper::run().as_code();
    }
    if mode == Mode::SweepTasks {
        return sweep::windows_sweeper::run().as_code();
    }
    if mode == Mode::WslDeny {
        return refresh::run_wsl_deny().as_code();
    }
    if mode == Mode::WslUpgrade {
        return refresh::run_wsl_upgrade().as_code();
    }
    super::event_trace::install();
    run_mode(mode).as_code()
}

/// Every mode but `--refresh`, `--stop`, `--sweep-tasks`, `--wsl-deny` and `--wsl-upgrade`,
/// which [`main`] runs.
fn run_mode(mode: Mode) -> SetupExit {
    match mode {
        Mode::SetUp {
            parent,
            data_dir,
            add,
        } => match Owner::of_parent(Some(parent)) {
            Ok(owner) => run_set_up(Names::production(), owner, &data_dir, add),
            Err(error) => {
                tracing::error!(%error, "the user who asked for the set-up can't be found");
                SetupExit::Failed
            }
        },
        Mode::Reclaim { parent } => match Owner::of_parent(Some(parent)) {
            Ok(owner) => run_reclaim(&owner, RECORD_KEY),
            Err(error) => {
                tracing::error!(%error, "the user who asked for the clean-up can't be found");
                SetupExit::Failed
            }
        },
        Mode::Adopt {
            user_install_dir,
            user,
        } => {
            let target = match user {
                Some(user) => Target::User {
                    owner_sid: user.owner_sid,
                    data_dir: user.data_dir,
                },
                None => Target::Machine,
            };
            run_adopt(Names::production(), &user_install_dir, &target)
        }
        Mode::Remove {
            per_machine,
            uninstall,
        } => {
            if per_machine && !may_remove_per_machine() {
                tracing::error!("--per-machine needs an elevated process or LOCAL SYSTEM");
                return SetupExit::Usage;
            }
            let invoker = Owner::of_parent(None)
                .inspect_err(|error| tracing::warn!(%error, "the user who asked isn't known"))
                .ok();
            run_remove(Names::production(), invoker, per_machine, uninstall)
        }
        Mode::Refresh | Mode::Stop | Mode::SweepTasks | Mode::WslDeny | Mode::WslUpgrade => {
            SetupExit::Usage
        }
    }
}

/// `--set-up` (or an extend, `add` above 0) on the machine `names` names, for `owner`, whose
/// data folder is `data_dir`; `setup-result.json` written as the owner, whatever happened.
pub(crate) fn run_set_up(names: Names, owner: Owner, given: &Path, add: u8) -> SetupExit {
    // The owner the request names is the verified parent's token user, and nothing else.
    let owner_sid = owner.sid().clone();
    // The folder is written as admin into the record's `DataDir`: resolved as the owner, and
    // the owner's, or refused before anything is touched or written, the result file included.
    let data_dir = match owners_data_dir(&owner, given) {
        Ok(data_dir) => data_dir,
        Err((exit, error)) => {
            tracing::error!(%error, "the data folder isn't the owner's own local folder");
            return exit;
        }
    };
    let data_dir = data_dir.as_path();
    let around = Box::new(Around::new(data_dir.to_owned()));
    let mut machine = WindowsMachine::new(names, owner, data_dir.to_owned(), around);
    let outcome = request_and_run(&mut machine, &owner_sid, data_dir, add);
    let exit = match &outcome {
        Ok(_) => SetupExit::Done,
        Err(failure) => failure.exit,
    };
    let result = SetupResult::of(&outcome);
    let written = owned(&mut machine, |_| {
        result_file::write(data_dir, &result)
            .map_err(|error| MachineError::Other(error.to_string()))
    });
    if let Err(error) = written {
        tracing::error!(%error, "setup-result.json couldn't be written");
    }
    exit
}

/// The request for `owner`, checked, then the set-up, which reads the registered matters
/// itself once it holds the lock (`Machine::registered_matters`).
fn request_and_run(
    machine: &mut WindowsMachine,
    owner: &Sid,
    data_dir: &Path,
    add: u8,
) -> Result<SetupReport, SetupFailure> {
    let req = SetupRequest {
        owner: owner.clone(),
        data_dir: data_dir.to_owned(),
        add,
    };
    if let Some(failure) = owner_failure(&req, owner) {
        return Err(failure);
    }
    let mut acl = NtfsAcl::new().map_err(|error| SetupFailure {
        step: Step::SharedWrites,
        cause: format!("this process's token: {error}"),
        rolled_back: true,
        exit: SetupExit::Failed,
    })?;
    set_up(machine, &mut acl, &req)
}

/// The data folder `given`, resolved as `owner` (`private_dir::resolve_folder`: no junction or
/// link below the drive's root, a folder at the end) and owned by `owner`, or by Administrators
/// when `owner` is a member (`sids::data_dir_owner_allowed`): its final path, which is what is
/// recorded and used from then on. Otherwise the exit and why: one owned by anyone else
/// (SYSTEM included) is [`SetupExit::DataDirNotOwned`]; one that can't be resolved so is `Usage`.
pub(super) fn owners_data_dir(owner: &Owner, given: &Path) -> Result<PathBuf, (SetupExit, String)> {
    let usage = |error: String| (SetupExit::Usage, error);
    let (canonical, folder_owner) = owner
        .run_as(|| resolve_folder(given))
        .map_err(|error| usage(error.to_string()))?
        .map_err(|error| usage(error.to_string()))?;
    let in_admins = || owner.in_administrators();
    if !data_dir_owner_allowed(&folder_owner, owner.sid(), in_admins) {
        return Err((
            SetupExit::DataDirNotOwned,
            format!(
                "{} belongs to {folder_owner}, not to {}, the user who asked",
                canonical.display(),
                owner.sid()
            ),
        ));
    }
    Ok(canonical)
}

/// Whether this process may run `--per-machine`: elevated, or LOCAL SYSTEM
/// (`remove::per_machine_allowed`). A token that can't be read may not.
fn may_remove_per_machine() -> bool {
    match this_process() {
        Ok((elevated, system)) => per_machine_allowed(elevated, system),
        Err(error) => {
            tracing::error!(%error, "this process's token can't be read");
            false
        }
    }
}

/// `--remove` (`per_machine`: `--per-machine`) on the machine `names` names, for `invoker`,
/// the verified parent's token user when found: [`remove`] over a [`WindowsRemoval`], its
/// outcome logged with counts and causes only.
pub(crate) fn run_remove(
    names: Names,
    invoker: Option<Owner>,
    per_machine: bool,
    uninstall: bool,
) -> SetupExit {
    let mut acl = match NtfsAcl::new() {
        Ok(acl) => acl,
        Err(error) => {
            tracing::error!(%error, "this process's token can't be read; nothing was removed");
            return SetupExit::Failed;
        }
    };
    let record_key = names.record_key.clone();
    let invoker_sid = invoker.as_ref().map(|owner| owner.sid().clone());
    let mut machine = WindowsRemoval::new(names, invoker);
    // The admin copy goes while the set-up lock is still held (`remove_then`), so a refresh
    // skips. When the teardown doesn't run (a per-user removal by someone else), nothing goes.
    let mut taken = Ok(());
    let mut teardown = |_: &Removed| {
        taken =
            remove_admin_copy_if_theirs(&record_key, uninstall, per_machine, invoker_sid.as_ref());
    };
    match remove_then(&mut machine, &mut acl, per_machine, &mut teardown) {
        Ok(removed) => {
            tracing::info!(?removed, "the removal finished");
            removal_exit(taken, &mut |line| refresh::event_log(true, line))
        }
        Err(failure) => {
            tracing::error!(step = %failure.step, cause = %failure.cause, "the removal stopped");
            failure.exit
        }
    }
}

/// The admin copy's removal (`admin_remove.rs`), when `admin_copy::copy_removable` says so:
/// the uninstaller's removal (`uninstall`; the app's in-app one never takes it), the pool under
/// `record_key` has no owner left, and this removal is the one of the install that placed the
/// copy (`AdminCopyOwner`): a per-machine one for `machine`, the lawyer's own
/// per-user one (`invoker`) for theirs. A copy that stays, or a record that can't be read to
/// tell, is the step that failed and why.
fn remove_admin_copy_if_theirs(
    record_key: &str,
    uninstall: bool,
    per_machine: bool,
    invoker: Option<&Sid>,
) -> Result<(), CopyLeft> {
    let read = super::record_write::read(record_key)
        .and_then(|pool| Ok((pool, super::record_write::read_admin(record_key)?)));
    match read {
        Ok((pool, (copy_owner, _version))) => {
            let removal = Removal {
                uninstall,
                per_machine,
                invoker,
            };
            if copy_removable(&removal, pool.is_some(), copy_owner.as_ref()) {
                remove_admin_copy(record_key).inspect_err(|left| {
                    let error = &left.cause.text;
                    tracing::error!(%error, "the admin copy couldn't be removed in full");
                })
            } else {
                tracing::info!("the admin copy isn't this removal's to take; it stays");
                Ok(())
            }
        }
        Err(error) => {
            tracing::error!(%error, "the record can't be read; the admin copy stays");
            Err(CopyLeft {
                step: "record",
                cause: Cause::from(error),
            })
        }
    }
}

#[cfg(test)]
mod tests;
