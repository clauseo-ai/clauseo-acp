//! The packs folder listed once ([`PackFolders`]), so a caller reading many packs (the
//! `ListSkillPacks` query) finds each pack's folder in that one listing instead of listing the
//! folder again per pack, which made the list quadratic in the number of folders.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::Refusal;

/// One listing of a packs folder: every entry `read_dir` gave, by its exact name, so a disk
/// that ignores case never matches `NDA-Kit` to `nda-kit`. A folder that could not be listed
/// holds none and answers every lookup with `missing`; an entry that could not be read makes
/// a lookup that finds nothing answer `io`, as a scan that met it would have.
#[derive(Debug, Clone, Default)]
pub struct PackFolders {
    entries: BTreeMap<OsString, Entry>,
    unlisted: Option<Refusal>,
    broken: bool,
}

#[derive(Debug, Clone)]
struct Entry {
    path: PathBuf,
    folder: bool,
}

impl PackFolders {
    /// Lists `packs_root` once. Off the host thread.
    pub fn list(packs_root: &Path) -> Self {
        let mut folders = Self::default();
        let Ok(entries) = std::fs::read_dir(packs_root) else {
            folders.unlisted = Some(Refusal::Missing);
            return folders;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                folders.broken = true;
                continue;
            };
            let folder = entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() || kind.is_symlink());
            let path = entry.path();
            folders
                .entries
                .insert(entry.file_name(), Entry { path, folder });
        }
        folders
    }

    /// `packs_root/<id>`, when the listing holds exactly that name.
    pub fn find(&self, id: &str) -> Result<&Path, Refusal> {
        if let Some(refusal) = self.unlisted {
            return Err(refusal);
        }
        match self.entries.get(OsStr::new(id)) {
            Some(entry) => Ok(&entry.path),
            None if self.broken => Err(Refusal::Io),
            None => Err(Refusal::Missing),
        }
    }

    /// The names of the entries that are folders, or links (refused as such when read), in
    /// name order.
    pub fn folder_names(&self) -> impl Iterator<Item = &OsStr> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.folder)
            .map(|(name, _)| name.as_os_str())
    }
}
