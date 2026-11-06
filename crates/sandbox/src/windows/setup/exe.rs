//! The setup exe's work (S15 `PR4h`), which `src/bin/clauseo-sandbox-setup.rs` runs elevated:
//! the command line (`args.rs`), then the mode.
//!
//! `--set-up` takes the owner from the verified parent (`Owner::of_parent`, D-B2), finds the
//! registered matters in the owner's files (`registered.rs`), builds the request for that owner
//! and checks it names that owner ([`owner_failure`]), then runs [`set_up`] over a
//! [`WindowsMachine`] and writes `setup-result.json` as the owner. Every read and write under
//! `<data_dir>\sandbox\` runs as the owner (`Machine::as_owner`). An owner that can't be found
//! writes no result (there is no one to write it as); the app then names the exit code.
//!
//! `--reclaim` is `PR4i`'s and `--remove` `PR4j`'s. Until then `--reclaim` fails, and
//! `--remove` exits 0 only when the machine has no record (nothing was set up), so the
//! uninstaller keeps `sandbox\` whenever a pool may still need its records.

use std::ffi::OsString;
use std::path::Path;

use super::args::{self, Mode};
use super::around::Around;
use super::machine::{Machine, MachineError, owned};
use super::outcome::{SetupExit, SetupFailure, SetupReport, Step};
use super::owner::Owner;
use super::record_write::{self, RECORD_KEY};
use super::registered::registered;
use super::result_file::{self, SetupResult};
use super::windows_machine::{Names, WindowsMachine};
use super::{SetupRequest, owner_failure, set_up};
use crate::windows::NtfsAcl;
use crate::windows::fence::RegisteredMatter;
use crate::windows::matters::MatterSids;
use crate::windows::sids::Sid;

/// Runs the exe on `args` (those after its name) and returns its exit.
pub fn main(args: impl IntoIterator<Item = OsString>) -> SetupExit {
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
        Mode::Remove => remove(RECORD_KEY),
    }
}

/// `--set-up` (or an extend, `add` above 0) on the machine `names` names, for `owner`, whose
/// data folder is `data_dir`; `setup-result.json` written as the owner, whatever happened.
pub(crate) fn run_set_up(names: Names, owner: Owner, data_dir: &Path, add: u8) -> SetupExit {
    // The owner the request names is the verified parent's token user, and nothing else.
    let owner_sid = owner.sid().clone();
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

/// The request for `owner`, checked, then the set-up.
fn request_and_run(
    machine: &mut WindowsMachine,
    owner: &Sid,
    data_dir: &Path,
    add: u8,
) -> Result<SetupReport, SetupFailure> {
    let failed = |step: Step, cause: String| SetupFailure {
        step,
        cause,
        rolled_back: true,
        exit: SetupExit::Failed,
    };
    let matters = owned(machine, |m| registered_matters(m, data_dir))
        .map_err(|error| failed(Step::Fence, format!("the registered matters: {error}")))?;
    let req = SetupRequest {
        owner: owner.clone(),
        data_dir: data_dir.to_owned(),
        add,
        matters,
    };
    if let Some(failure) = owner_failure(&req, owner) {
        return Err(failure);
    }
    let mut acl = NtfsAcl::new()
        .map_err(|error| failed(Step::SharedWrites, format!("this process's token: {error}")))?;
    set_up(machine, &mut acl, &req)
}

/// The registered matters in the owner's `matters.json` and ledger, trusting the pool and group
/// the machine's record names. The caller acts as the owner.
fn registered_matters(
    machine: &mut WindowsMachine,
    data_dir: &Path,
) -> Result<Vec<RegisteredMatter>, MachineError> {
    let matters =
        MatterSids::load(data_dir).map_err(|error| MachineError::Other(error.to_string()))?;
    if matters.is_empty() {
        return Ok(Vec::new());
    }
    let mut trusted = Vec::new();
    if let Some(record) = machine.read_record()? {
        trusted.extend(record.pool);
        trusted.push(record.group);
    }
    let ledger = machine
        .ledger(&trusted)
        .map_err(|error| MachineError::Other(format!("the permission ledger: {error}")))?;
    Ok(registered(matters.rows(), &ledger))
}

/// `--remove` until `PR4j`: 0 when `HKLM\<key>` is missing (nothing was set up, so nothing
/// needs `sandbox\`'s records), else [`SetupExit::Failed`], which keeps them.
fn remove(key: &str) -> SetupExit {
    match record_write::read(key) {
        Ok(None) => SetupExit::Done,
        Ok(Some(_)) => {
            tracing::error!("removing a pool isn't built yet (S15 PR4j); its records are kept");
            SetupExit::Failed
        }
        Err(error) => {
            tracing::error!(%error, "the machine's record can't be read; its records are kept");
            SetupExit::Failed
        }
    }
}

#[cfg(test)]
mod tests;
