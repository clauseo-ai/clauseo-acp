//! The tests' stand-in for NTFS on other OSes: inode numbers for file IDs, the owner's uid,
//! and renames that refuse a taken name and a symbolic link on the way.

use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

use super::super::SetAsideFailure;
use super::super::files::FileId;
use super::super::run::Volume;
use crate::windows::write::{make_sandbox_dir, rename_new};

pub(super) struct Posix;

impl Volume for Posix {
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
        parts: &[String],
        name: &str,
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
        parts: &[String],
        name: &str,
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
