//! [`Adopter`] on Windows: the admin copy's folder and file, the machine record and the set-up
//! in this process, as `exe::main` runs `--adopt`.

use std::path::{Path, PathBuf};

use super::{Adopter, Target, adopt};
use crate::windows::admin_copy::{Found, os};
use crate::windows::setup::exe::run_set_up;
use crate::windows::setup::outcome::SetupExit;
use crate::windows::setup::owner::{Owner, this_process};
use crate::windows::setup::profile::profile_of;
use crate::windows::setup::record_write;
use crate::windows::setup::windows_machine::Names;
use crate::windows::sids::Sid;

/// The machine `--adopt` changes: the record under `names.record_key`, and the installing
/// user once found (the recorded parent's token user).
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

impl Adopter for WindowsAdopter {
    fn program_files(&mut self) -> Result<PathBuf, String> {
        os::program_files().map_err(|error| error.to_string())
    }

    fn current_exe(&mut self) -> Result<PathBuf, String> {
        std::env::current_exe().map_err(|error| error.to_string())
    }

    fn token(&mut self) -> Result<(bool, bool), String> {
        this_process().map_err(|error| error.to_string())
    }

    fn installing_user(&mut self) -> Result<Sid, String> {
        let owner = Owner::of_parent(None).map_err(|error| error.to_string())?;
        let sid = owner.sid().clone();
        self.user = Some(owner);
        Ok(sid)
    }

    fn record_owner(&mut self) -> Result<Option<Sid>, String> {
        record_write::read(&self.names.record_key)
            .map(|record| record.map(|record| record.owner))
            .map_err(|error| error.to_string())
    }

    /// A temporary profile counts as none: its folder isn't the user's own.
    fn profile_of(&mut self, sid: &Sid) -> Result<Option<PathBuf>, String> {
        let profile = profile_of(sid).map_err(|error| error.to_string())?;
        Ok(profile
            .filter(|profile| !profile.temporary)
            .map(|profile| profile.path))
    }

    fn is_folder(&mut self, path: &Path) -> bool {
        matches!(os::found(path), Ok(Found::Folder))
    }

    fn lock_down(&mut self, dir: &Path, exe: &Path) -> Result<(), String> {
        if !matches!(os::found(dir), Ok(Found::Folder))
            || !matches!(os::found(exe), Ok(Found::File))
        {
            return Err("the admin folder or copy isn't a plain folder and file".to_owned());
        }
        os::lock_down(dir, true).map_err(|error| format!("the folder: {error}"))?;
        os::lock_down(exe, false).map_err(|error| format!("the copy: {error}"))
    }

    fn sha256(&mut self, exe: &Path) -> Result<String, String> {
        os::sha256(exe).map_err(|error| error.to_string())
    }

    fn write_values(&mut self, sha256: &str, install_dir: &Path) -> Result<(), String> {
        record_write::write_admin_values(&self.names.record_key, sha256, install_dir)
            .map_err(|error| error.to_string())
    }

    fn set_up(&mut self, data_dir: &Path) -> SetupExit {
        let Some(owner) = self.user.take() else {
            tracing::error!("--adopt: the set-up has no installing user");
            return SetupExit::Failed;
        };
        let names = Names {
            record_key: self.names.record_key.clone(),
            group: self.names.group.clone(),
            mutex: self.names.mutex.clone(),
            users: self.names.users.clone(),
            journals: self.names.journals,
        };
        run_set_up(names, owner, data_dir, 0)
    }
}
