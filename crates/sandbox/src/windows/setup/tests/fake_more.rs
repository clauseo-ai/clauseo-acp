//! More of the set-up's fakes (split from `fake.rs`): the surroundings the fence's walks get,
//! how the call lines name a folder, and the profile faults the reset tests inject.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use crate::windows::FolderSupport;
use crate::windows::admit::{FolderList, ReadDir};
use crate::windows::apply::Surroundings;

/// A folder as the call lines name it: a profile's `LocalLow` by its account
/// (`ClauseoSb01`), anything else by its last part.
pub(super) fn folder_name(path: &Path) -> String {
    let account = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name);
    match (path.file_name(), account) {
        (Some(last), Some(account)) if last == "LocalLow" => account.to_string_lossy().into_owned(),
        _ => path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
    }
}

/// The surroundings: every folder `Ok`, the real listing, no set-aside, no credentials.
#[derive(Debug, Default)]
pub(super) struct Around;

impl FolderList for Around {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        ReadDir.subfolders(parent)
    }
}

impl Surroundings for Around {
    fn check(&self, _folder: &Path) -> io::Result<FolderSupport> {
        Ok(FolderSupport::Ok)
    }

    fn set_aside(&mut self, _matter: clauseo_protocol::MatterId, _path: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn credentials(&self) -> Option<PathBuf> {
        None
    }
}

/// The profile faults the reset tests inject (`resets.rs`), and the folders deleted.
#[derive(Debug, Default)]
pub(super) struct ProfileFaults {
    /// Accounts whose profile folder survives `delete_profile`.
    pub(super) stuck: BTreeSet<String>,
    /// Accounts whose logon makes `<name>.HOST` instead of `<name>`.
    pub(super) moved: BTreeSet<String>,
    /// The profile folders `delete_profile` deleted, until a logon makes them again.
    gone: BTreeSet<PathBuf>,
}

impl ProfileFaults {
    /// The folder under `Users` a logon of `name` makes.
    pub(super) fn folder(&self, name: &str) -> String {
        if self.moved.contains(name) {
            format!("{name}.HOST")
        } else {
            name.to_owned()
        }
    }

    /// `name`'s profile at `path` was deleted: gone, unless its folder is stuck.
    pub(super) fn deleted(&mut self, name: Option<&String>, path: PathBuf) {
        if !name.is_some_and(|name| self.stuck.contains(name)) {
            self.gone.insert(path);
        }
    }

    /// A logon made the profile at `path` again.
    pub(super) fn made(&mut self, path: &Path) {
        self.gone.remove(path);
    }

    /// Whether `path` is in a deleted profile.
    pub(super) fn is_gone(&self, path: &Path) -> bool {
        self.gone.iter().any(|folder| path.starts_with(folder))
    }
}
