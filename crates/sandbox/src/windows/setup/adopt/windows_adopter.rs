//! [`Adopter`] on Windows: the admin copy's folder and file, the machine record and the set-up
//! in this process, as `exe::main` runs `--adopt`.

use std::path::{Path, PathBuf};

use super::{Adopter, Records, Target, adopt};
use crate::windows::admin_copy::{Found, os};
use crate::windows::refresh::{Starters, SystemTask, TaskError, event_log};
use crate::windows::setup::exe::{owners_data_dir, run_set_up};
use crate::windows::setup::explain::{AdoptFailure, AdoptStep, Cause, Code};
use crate::windows::setup::lock::{Held, try_lock};
use crate::windows::setup::outcome::SetupExit;
use crate::windows::setup::owner::{Owner, this_process};
use crate::windows::setup::profile::profile_of;
use crate::windows::setup::record_values::AdminValues;
use crate::windows::setup::record_write;
use crate::windows::setup::result_file;
use crate::windows::setup::windows_machine::Names;
use crate::windows::sids::Sid;

/// The machine `--adopt` changes: the record under `names.record_key`, and the lawyer once
/// found (`Owner::of_installer`: the unelevated installer's token user).
pub(crate) struct WindowsAdopter {
    names: Names,
    user: Option<Owner>,
    /// The set-up lock, while `--adopt` holds it.
    held: Option<Held>,
    /// The lawyer named and the data folder, per user: where a failure's result is written.
    target: Option<(Sid, PathBuf)>,
}

impl WindowsAdopter {
    /// The machine with the names `names` (`Names::production()` for a real install).
    pub(crate) fn new(names: Names) -> WindowsAdopter {
        WindowsAdopter {
            names,
            user: None,
            held: None,
            target: None,
        }
    }
}

/// `--adopt` on this machine: [`adopt`] over a [`WindowsAdopter`].
pub(crate) fn run_adopt(names: Names, install_dir: &Path, target: &Target) -> SetupExit {
    let mut machine = WindowsAdopter::new(names);
    if let Target::User {
        owner_sid,
        data_dir,
        ..
    } = target
    {
        machine.target = Some((owner_sid.clone(), data_dir.clone()));
    }
    adopt(&mut machine, install_dir, target)
}

/// `sid`'s own profile folder, as `ProfileList` names it (`profile_of`); a temporary profile
/// counts as none, as its folder isn't the user's own. `--adopt` and the refresh task (PR 3)
/// both check the install folder against it.
pub(crate) fn own_profile(sid: &Sid) -> Result<Option<PathBuf>, String> {
    let profile = profile_of(sid).map_err(|error| error.to_string())?;
    Ok(profile
        .filter(|profile| !profile.temporary)
        .map(|profile| profile.path))
}

impl Adopter for WindowsAdopter {
    fn program_files(&mut self) -> Result<PathBuf, Cause> {
        os::program_files().map_err(Cause::from)
    }

    fn running_image(&mut self) -> Result<PathBuf, Cause> {
        os::running_image().map_err(Cause::from)
    }

    fn token(&mut self) -> Result<(bool, bool), Cause> {
        this_process().map_err(Cause::from)
    }

    fn installing_user(&mut self, claimed: &Sid) -> Result<Sid, Cause> {
        let owner = Owner::of_installer(claimed)?;
        let sid = owner.sid().clone();
        self.user = Some(owner);
        Ok(sid)
    }

    fn records(&mut self) -> Result<Records, Cause> {
        let key = &self.names.record_key;
        let pool = record_write::read(key)?;
        let (copy_owner, version) = record_write::read_admin(key)?;
        Ok(Records {
            group: pool.as_ref().map(|record| record.group.clone()),
            pool_owner: pool.map(|record| record.owner),
            copy_owner,
            version,
        })
    }

    fn profile_of(&mut self, sid: &Sid) -> Result<Option<PathBuf>, Cause> {
        let profile = profile_of(sid)?;
        Ok(profile
            .filter(|profile| !profile.temporary)
            .map(|profile| profile.path))
    }

    /// One that can't be read counts as missing.
    fn found(&mut self, path: &Path) -> Found {
        os::found(path).unwrap_or(Found::Missing)
    }

    fn security(&mut self, path: &Path) -> Result<Vec<u8>, Cause> {
        os::security(path).map_err(Cause::from)
    }

    fn lock_down(&mut self, dir: &Path, exe: &Path) -> Result<(), Cause> {
        os::lock_down_tree(dir, exe).map_err(Cause::from)
    }

    fn sha256(&mut self, exe: &Path) -> Result<String, Cause> {
        os::sha256(exe).map_err(Cause::from)
    }

    fn lock(&mut self) -> Result<bool, Cause> {
        self.held = try_lock(&self.names.mutex)?;
        Ok(self.held.is_some())
    }

    fn unlock(&mut self) {
        self.held = None;
    }

    fn write_values(&mut self, values: &AdminValues) -> Result<(), Cause> {
        record_write::write_admin_values(&self.names.record_key, values).map_err(Cause::from)
    }

    fn register_task(&mut self, task: SystemTask, starters: &Starters) -> Result<(), Cause> {
        crate::windows::refresh::register_task(task, starters).map_err(|error| task_cause(&error))
    }

    fn set_up(&mut self, data_dir: &Path) -> SetupExit {
        let Some(owner) = self.user.take() else {
            tracing::error!("--adopt: the set-up has no installing user");
            self.report(&AdoptFailure {
                step: AdoptStep::SetUpUser,
                cause: Cause::from("no installing user"),
            });
            return SetupExit::Failed;
        };
        run_set_up(self.names.clone(), owner, data_dir, 0)
    }

    /// The Event Log's line always; the result file only for the lawyer the installer named,
    /// confirmed (`installing_user`), in their own data folder (`owners_data_dir`), written as
    /// them, as step 8's is. Before that there is no one to write it as.
    fn report(&mut self, failure: &AdoptFailure) {
        event_log(true, &failure.line());
        let (Some(owner), Some((named, given))) = (&self.user, &self.target) else {
            return;
        };
        if owner.sid() != named {
            return;
        }
        let written = owners_data_dir(owner, given)
            .map_err(|(_, error)| error)
            .and_then(|data_dir| {
                owner
                    .run_as(|| result_file::write(&data_dir, &failure.result()))
                    .map_err(|error| error.to_string())?
                    .map_err(|error| error.to_string())
            });
        if let Err(error) = written {
            tracing::error!(%error, "--adopt: setup-result.json couldn't be written");
        }
    }
}

/// A task call's error: a COM one gives its HRESULT.
fn task_cause(error: &TaskError) -> Cause {
    match error {
        TaskError::Com { hresult, .. } => Cause::with(error.to_string(), Code::Hresult(*hresult)),
        TaskError::NotRegistered
        | TaskError::Path(_)
        | TaskError::Folder(_)
        | TaskError::Descriptor(_) => Cause::from(error.to_string()),
    }
}
