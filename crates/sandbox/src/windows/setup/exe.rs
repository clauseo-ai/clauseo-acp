//! The setup exe's work (S15 `PR4h`), which `src/bin/clauseo-sandbox-setup.rs` runs elevated:
//! the command line (`args.rs`), then the mode.
//!
//! `--set-up` takes the owner from the verified parent (`Owner::of_parent`, D-B2), and resolves
//! `--data-dir` as the owner: no junction or link below the drive's root, a folder the owner
//! owns, recorded and used as its final path from then on (a `subst` drive resolves to the real
//! folder). One owned by anyone else exits `DataDirNotOwned`, anything else `Usage`, with
//! nothing touched. It builds
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
//! writes no result file: the hook reads only the exit code. `--reclaim` is `PR4i`'s; until
//! then it fails.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::args::{self, Mode};
use super::around::Around;
use super::ffi::restrict_dll_search;
use super::machine::{MachineError, owned};
use super::outcome::{SetupExit, SetupFailure, SetupReport, Step};
use super::owner::{Owner, this_process};
use super::remove::{WindowsRemoval, per_machine_allowed, remove};
use super::result_file::{self, SetupResult};
use super::windows_machine::{Names, WindowsMachine};
use super::{SetupRequest, owner_failure, set_up};
use crate::windows::NtfsAcl;
use crate::windows::private_dir::resolve_folder;
use crate::windows::sids::Sid;

/// Runs the exe on `args` (those after its name) and returns its exit.
pub fn main(args: impl IntoIterator<Item = OsString>) -> SetupExit {
    // First: no DLL from beside the exe (a folder a standard user may write) loads from here on.
    if let Err(error) = restrict_dll_search() {
        tracing::error!(%error, "the DLL search couldn't be kept to System32");
        return SetupExit::Failed;
    }
    let mode = match args::parse(args) {
        Ok(mode) => mode,
        Err(usage) => {
            tracing::error!(%usage, "the set-up was started wrongly");
            return SetupExit::Usage;
        }
    };
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
        Mode::Reclaim { .. } => {
            tracing::error!("--reclaim isn't built yet (S15 PR4i)");
            SetupExit::Failed
        }
        Mode::Remove { per_machine } => {
            if per_machine && !may_remove_per_machine() {
                tracing::error!("--per-machine needs an elevated process or LOCAL SYSTEM");
                return SetupExit::Usage;
            }
            let invoker = Owner::of_parent(None)
                .inspect_err(|error| tracing::warn!(%error, "the user who asked isn't known"))
                .ok();
            run_remove(Names::production(), invoker, per_machine)
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
/// link below the drive's root, a folder at the end) and owned by `owner`: its final path, which
/// is what is recorded and used from then on. Otherwise the exit and why: one owned by anyone
/// else is [`SetupExit::DataDirNotOwned`]; one that can't be resolved so is `Usage`.
fn owners_data_dir(owner: &Owner, given: &Path) -> Result<PathBuf, (SetupExit, String)> {
    let usage = |error: String| (SetupExit::Usage, error);
    let (canonical, folder_owner) = owner
        .run_as(|| resolve_folder(given))
        .map_err(|error| usage(error.to_string()))?
        .map_err(|error| usage(error.to_string()))?;
    if folder_owner != *owner.sid() {
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
pub(crate) fn run_remove(names: Names, invoker: Option<Owner>, per_machine: bool) -> SetupExit {
    let mut acl = match NtfsAcl::new() {
        Ok(acl) => acl,
        Err(error) => {
            tracing::error!(%error, "this process's token can't be read; nothing was removed");
            return SetupExit::Failed;
        }
    };
    let mut machine = WindowsRemoval::new(names, invoker);
    match remove(&mut machine, &mut acl, per_machine) {
        Ok(removed) => {
            tracing::info!(?removed, "the removal finished");
            SetupExit::Removed
        }
        Err(failure) => {
            tracing::error!(step = %failure.step, cause = %failure.cause, "the removal stopped");
            failure.exit
        }
    }
}

#[cfg(test)]
mod tests;
