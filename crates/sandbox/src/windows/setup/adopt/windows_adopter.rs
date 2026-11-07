//! [`Adopter`] on Windows: the admin copy's folder and file, the machine record and the set-up
//! in this process, as `exe::main` runs `--adopt`.

use std::path::{Path, PathBuf};

use super::{Adopter, Records, Target, adopt};
use crate::windows::admin_copy::{Found, os};
use crate::windows::setup::exe::run_set_up;
use crate::windows::setup::outcome::SetupExit;
use crate::windows::setup::owner::{Owner, this_process};
use crate::windows::setup::profile::profile_of;
use crate::windows::setup::record_values::AdminValues;
use crate::windows::setup::record_write;
use crate::windows::setup::windows_machine::Names;
use crate::windows::sids::Sid;

/// The machine `--adopt` changes: the record under `names.record_key`, and the lawyer once
/// found (`Owner::of_installer`: the unelevated installer's token user).
pub(crate) struct WindowsAdopter {
    names: Names,
    user: Option<Owner>,
}

impl WindowsAdopter {
    /// The machine with the names `names` (`Names::production()` for a real install).
    pub(crate) fn new(names: Names) -> WindowsAdopter {
        WindowsAdopter { names, user: None }
    }
}

/// `--adopt` on this machine: [`adopt`] over a [`WindowsAdopter`].
pub(crate) fn run_adopt(names: Names, install_dir: &Path, target: &Target) -> SetupExit {
    adopt(&mut WindowsAdopter::new(names), install_dir, target)
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
    fn program_files(&mut self) -> Result<PathBuf, String> {
        os::program_files().map_err(|error| error.to_string())
    }

    fn running_image(&mut self) -> Result<PathBuf, String> {
        os::running_image().map_err(|error| error.to_string())
    }

    fn token(&mut self) -> Result<(bool, bool), String> {
        this_process().map_err(|error| error.to_string())
    }

    fn installing_user(&mut self, claimed: &Sid) -> Result<Sid, String> {
        let owner = Owner::of_installer(claimed).map_err(|error| error.to_string())?;
        let sid = owner.sid().clone();
        self.user = Some(owner);
        Ok(sid)
    }

    fn records(&mut self) -> Result<Records, String> {
        let key = &self.names.record_key;
        let pool = record_write::read(key).map_err(|error| error.to_string())?;
        let (copy_owner, version) =
            record_write::read_admin(key).map_err(|error| error.to_string())?;
        Ok(Records {
            pool_owner: pool.map(|record| record.owner),
            copy_owner,
            version,
        })
    }

    fn profile_of(&mut self, sid: &Sid) -> Result<Option<PathBuf>, String> {
        own_profile(sid)
    }

    /// One that can't be read counts as missing.
    fn found(&mut self, path: &Path) -> Found {
        os::found(path).unwrap_or(Found::Missing)
    }

    fn security(&mut self, path: &Path) -> Result<Vec<u8>, String> {
        os::security(path).map_err(|error| error.to_string())
    }

    fn lock_down(&mut self, dir: &Path, exe: &Path) -> Result<(), String> {
        os::lock_down_tree(dir, exe).map_err(|error| error.to_string())
    }

    fn sha256(&mut self, exe: &Path) -> Result<String, String> {
        os::sha256(exe).map_err(|error| error.to_string())
    }

    fn write_values(&mut self, values: &AdminValues) -> Result<(), String> {
        record_write::write_admin_values(&self.names.record_key, values)
            .map_err(|error| error.to_string())
    }

    fn set_up(&mut self, data_dir: &Path) -> SetupExit {
        let Some(owner) = self.user.take() else {
            tracing::error!("--adopt: the set-up has no installing user");
            return SetupExit::Failed;
        };
        run_set_up(self.names.clone(), owner, data_dir, 0)
    }
}
