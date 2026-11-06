//! More of the set-up's fakes (split from `fake.rs`): the surroundings the fence's walks get,
//! and how the call lines name a folder.

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
