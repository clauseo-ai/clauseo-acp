//! The tests' stand-in for NTFS on other OSes: inode numbers for file IDs, the owner's uid,
//! renames that refuse a taken name and a symbolic link on the way, and a scan by path that
//! never follows a symbolic link (names match exactly, the stand-in's rule).

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use super::super::SetAsideFailure;
use super::super::files::FileId;
use super::super::run::Volume;
use super::super::scan::{Kind, Listed};
use crate::windows::write::{make_sandbox_dir, rename_new};

pub(super) struct Posix;

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
        match std::fs::symlink_metadata(path) {
            Ok(meta) => Ok(Some(FileId {
                volume: meta.dev(),
                id: u128::from(meta.ino()).to_le_bytes(),
            })),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn owner(&self, path: &Path) -> io::Result<String> {
        Ok(format!("uid {}", std::fs::symlink_metadata(path)?.uid()))
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
        moved(from, &folder.join(name))
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
        moved(&from.join(name), to)
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
