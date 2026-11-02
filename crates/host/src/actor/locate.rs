//! What the desktop's `open_file` and `reveal` ask the host
//! (`docs/decisions/0026-open-and-reveal.md`): the file a ref names, checked at call time against
//! the matter that owns it, and a matter's folder.

use std::path::PathBuf;

use clauseo_protocol::{ErrorCode, FileRefId, MatterId};

use super::Actor;
use crate::LocatedFile;
use crate::error::Error;
use crate::file_link;

/// The file `file` names, with the matter that owns it and that matter's folder. The owner is
/// the matter whose recorded folder the path is written under (the deepest, were folders
/// nested); the check that the file lies in that folder runs now, on the paths as they resolve
/// (`file_link::inside`), since the folder or the file may have moved since the ref was minted.
/// `command.invalid` for a ref this run did not issue and for a file in no matter's folder or
/// outside its owner's; `matter.folder_missing` when the owner's folder is gone.
pub(super) fn locate_file(actor: &Actor, file: FileRefId) -> Result<LocatedFile, Error> {
    let path = actor.files.get(&file).ok_or_else(|| {
        Error::refused(
            ErrorCode::CommandInvalid,
            format!("file ref {file} was not issued in this run"),
        )
    })?;
    let (matter, folder) = actor
        .matters
        .iter()
        .map(|(id, entry)| (*id, PathBuf::from(&entry.row.folder_path)))
        .filter(|(_, folder)| path.starts_with(folder))
        .max_by_key(|(_, folder)| folder.components().count())
        .ok_or_else(|| {
            Error::refused(
                ErrorCode::CommandInvalid,
                format!("file ref {file} names a file in no matter's folder"),
            )
        })?;
    if !folder.is_dir() {
        return Err(Error::refused(
            ErrorCode::MatterFolderMissing,
            format!("the folder of matter {matter} is not there"),
        ));
    }
    if !file_link::inside(path, &folder) {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            format!("file ref {file} names a file outside the folder of matter {matter}"),
        ));
    }
    Ok(LocatedFile {
        matter,
        folder,
        path: path.clone(),
    })
}

/// The folder recorded for `matter`: `command.unknown_matter` for a matter the host does not
/// know. Whether the folder is there is the caller's to check.
pub(super) fn matter_folder(actor: &Actor, matter: MatterId) -> Result<PathBuf, Error> {
    actor
        .check_matter(matter)
        .map(|entry| PathBuf::from(&entry.row.folder_path))
}
