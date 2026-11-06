//! The tests' stand-in for NTFS on other OSes: inode numbers for file IDs, the owner's uid,
//! renames that refuse a taken name and a symbolic link on the way, and a scan by path that
//! never follows a symbolic link. A name opened by path (a probe, an ID, an owner, a move's
//! source) that isn't there exactly is matched ignoring ASCII case, as NTFS would open it, so
//! a case-sensitive volume behaves as a case-insensitive one does; listings keep names as given.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use super::super::SetAsideFailure;
use super::super::files::FileId;
use super::super::run::Volume;
use super::super::scan::{Kind, Listed};
use crate::windows::write::{make_sandbox_dir, rename_new};

pub(super) struct Posix {
    /// Whether a name not there exactly is matched ignoring ASCII case; off only for the
    /// timing test on a case-insensitive volume, where the extra listing would be measured.
    pub(super) fold: bool,
    /// Items another user owns, by inode, with that owner: every file here is the test user's,
    /// so the stand-in keeps who else owns one here. [`Volume::owner`] says so for them.
    pub(super) foreign: RefCell<BTreeMap<u64, String>>,
    /// Each path [`Volume::own`] handed to the desktop user (one of `foreign`), in order.
    pub(super) owned: RefCell<Vec<PathBuf>>,
    /// The machine record's pool, as [`Volume::pool`] reads it: `None` is a record that can't
    /// be read. [`POOL_USER`] alone unless a test says otherwise.
    pub(super) pool: RefCell<Option<Vec<String>>>,
}

/// The owner [`Posix::foreign`] gives an item: a pool user the record names.
pub(super) const POOL_USER: &str = "uid other";

impl Posix {
    pub(super) fn new(fold: bool) -> Posix {
        Posix {
            fold,
            foreign: RefCell::default(),
            owned: RefCell::default(),
            pool: RefCell::new(Some(vec![POOL_USER.to_owned()])),
        }
    }

    /// Marks the item at `path` as a pool user's ([`POOL_USER`]).
    pub(super) fn foreign(&self, path: &Path) {
        self.foreign_as(path, POOL_USER);
    }

    /// Marks the item at `path` (and every name linked to it) as `owner`'s.
    pub(super) fn foreign_as(&self, path: &Path, owner: &str) {
        let ino = std::fs::symlink_metadata(path).unwrap().ino();
        self.foreign.borrow_mut().insert(ino, owner.to_owned());
    }

    /// `path` as this volume opens it: through [`on_disk`] when folding.
    fn at(&self, path: &Path) -> PathBuf {
        if self.fold {
            on_disk(path)
        } else {
            path.to_owned()
        }
    }
}

/// `path` as a folder to scan: one, not a link, and one this process may list, as NTFS's
/// open to list it would check.
fn listable(path: PathBuf) -> io::Result<PathBuf> {
    let meta = std::fs::symlink_metadata(&path)?;
    if !meta.is_dir() {
        return Err(io::Error::other("not a folder, or a link"));
    }
    std::fs::read_dir(&path)?;
    Ok(path)
}

/// `path` as NTFS would open it: itself when something is there, else the one name in its
/// folder that matches its last part ignoring ASCII case, else itself.
fn on_disk(path: &Path) -> PathBuf {
    if std::fs::symlink_metadata(path).is_ok() {
        return path.to_owned();
    }
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return path.to_owned();
    };
    let Some(name) = name.to_str() else {
        return path.to_owned();
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return path.to_owned();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .find(|found| found.to_str().is_some_and(|f| f.eq_ignore_ascii_case(name)))
        .map_or_else(|| path.to_owned(), |found| folder.join(found))
}

impl Volume for Posix {
    type Folder = PathBuf;

    fn open_root(&self, root: &Path) -> io::Result<PathBuf> {
        listable(root.to_owned())
    }

    fn open_folder(&self, folder: &PathBuf, name: &OsStr) -> io::Result<PathBuf> {
        listable(folder.join(name))
    }

    fn folder_id(&self, folder: &PathBuf) -> io::Result<FileId> {
        self.file_id(folder)?
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn list(&self, folder: &PathBuf) -> io::Result<Vec<Listed>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let kind = if kind.is_symlink() {
                Kind::Link
            } else if kind.is_dir() {
                Kind::Folder
            } else {
                Kind::Other
            };
            let name = entry.file_name();
            out.push(Listed { name, kind });
        }
        Ok(out)
    }

    fn probe(&self, folder: &PathBuf, name: &OsStr) -> io::Result<Option<FileId>> {
        self.file_id(&folder.join(name))
    }

    fn file_id(&self, path: &Path) -> io::Result<Option<FileId>> {
        match std::fs::symlink_metadata(self.at(path)) {
            Ok(meta) => Ok(Some(FileId {
                volume: meta.dev(),
                id: u128::from(meta.ino()).to_le_bytes(),
            })),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn owner(&self, path: &Path) -> io::Result<String> {
        let meta = std::fs::symlink_metadata(self.at(path))?;
        if let Some(owner) = self.foreign.borrow().get(&meta.ino()) {
            return Ok(owner.clone());
        }
        Ok(format!("uid {}", meta.uid()))
    }

    fn pool(&self) -> io::Result<Vec<String>> {
        self.pool
            .borrow()
            .clone()
            .ok_or_else(|| io::Error::from(io::ErrorKind::PermissionDenied))
    }

    /// Every item in `path`'s tree one of `pool` owns, never through a link, handed back; a
    /// file with another name too only when one of `pool` owns it, as the NTFS walk does.
    fn own(&self, path: &Path, pool: &[String]) -> io::Result<()> {
        let mut stack = vec![self.at(path)];
        while let Some(at) = stack.pop() {
            let meta = std::fs::symlink_metadata(&at)?;
            let mut foreign = self.foreign.borrow_mut();
            let pooled = foreign
                .get(&meta.ino())
                .is_some_and(|owner| pool.contains(owner));
            if pooled {
                foreign.remove(&meta.ino());
                self.owned.borrow_mut().push(at.clone());
            }
            drop(foreign);
            if meta.is_dir() {
                for entry in std::fs::read_dir(&at)? {
                    stack.push(entry?.path());
                }
            }
        }
        Ok(())
    }

    fn move_into(
        &self,
        from: &Path,
        sandbox: &Path,
        parts: &[OsString],
        name: &OsStr,
    ) -> Result<(), SetAsideFailure> {
        let mut folder = sandbox.to_owned();
        for part in parts {
            folder.push(part);
            if std::fs::symlink_metadata(&folder).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(SetAsideFailure::Changed);
            }
            make_sandbox_dir(&folder).map_err(|e| SetAsideFailure::Io(e.kind()))?;
        }
        moved(&self.at(from), &folder.join(name))
    }

    fn move_back(
        &self,
        sandbox: &Path,
        parts: &[OsString],
        name: &OsStr,
        to: &Path,
    ) -> Result<(), SetAsideFailure> {
        let from = parts
            .iter()
            .fold(sandbox.to_owned(), |p, part| p.join(part));
        moved(&self.at(&from.join(name)), to)
    }
}

fn moved(from: &Path, to: &Path) -> Result<(), SetAsideFailure> {
    if std::fs::symlink_metadata(from).is_err() {
        return Err(SetAsideFailure::Gone);
    }
    rename_new(from, to).map_err(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            SetAsideFailure::NameTaken
        } else {
            SetAsideFailure::Io(e.kind())
        }
    })
}
