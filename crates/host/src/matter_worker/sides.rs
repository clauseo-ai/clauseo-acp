//! The matter thread's part of a Word blackline (`MatterJob::WriteSides`): it copies the two
//! sides of one document into a fresh folder outside the matter, and nothing else. stetdoc runs
//! later, off the actor (`actor/query/blackline.rs`).
//!
//! It answers: where the copies go (`<dir>/old.docx` and `<dir>/new.docx`, in the one folder
//! `dir`, made here and private to the user on Unix); what each side is (`Matter::read_file` for
//! a version; for the folder now, a copy of the file itself, never through a link); what a
//! missing file answers (`Error::UnknownFile`, as `read_file` answers it); and how the folder
//! goes ([`BlacklineDir`], whose drop removes it on every path).

use std::path::{Path, PathBuf};

use clauseo_matters::{CompareSide, Matter, Settings};
use clauseo_protocol::{CheckpointId, ErrorCode, MatterPath};

use super::{MatterReply, Slot, open};
use crate::error::Error;

/// The older side's name in the folder.
pub(crate) const OLD: &str = "old.docx";
/// The newer side's name in the folder.
pub(crate) const NEW: &str = "new.docx";

/// A blackline's folder under `<data>/tmp/`, removed with everything in it when dropped:
/// after the answer, on an error, and when the work is dropped half way.
#[derive(Debug)]
pub(crate) struct BlacklineDir {
    path: PathBuf,
}

impl BlacklineDir {
    /// The folder.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for BlacklineDir {
    fn drop(&mut self) {
        if let Err(error) = crate::write::remove_tree(&self.path) {
            tracing::warn!(%error, "a Word blackline's temporary folder could not be removed");
        }
    }
}

/// The job: `path`'s copies from `from` and `to`, into the new folder `dir`.
#[derive(Debug)]
pub(crate) struct Sides {
    /// The older version.
    pub(crate) from: CheckpointId,
    /// The newer version, or the folder now.
    pub(crate) to: CompareSide,
    /// The document.
    pub(crate) path: MatterPath,
    /// The folder to make, under `<data>/tmp/`.
    pub(crate) dir: PathBuf,
}

impl Sides {
    /// Runs the job on the worker's matter, opening it first when needed ([`write_sides`]).
    pub(super) fn reply(self, settings: &Settings, slot: &mut Slot) -> MatterReply {
        MatterReply::SidesWritten(
            open(settings, slot)
                .and_then(|matter| write_sides(matter, self.from, self.to, &self.path, self.dir)),
        )
    }
}

/// Makes `dir` (its parent too, when missing) and writes `from`'s copy of `path` there as
/// [`OLD`] and `to`'s as [`NEW`]. A file the version or the folder lacks is
/// `Error::UnknownFile` (`command.invalid`); a copy that can't be written is
/// `blackline.failed`. On any error the folder is gone before this returns.
fn write_sides(
    matter: &Matter,
    from: CheckpointId,
    to: CompareSide,
    path: &MatterPath,
    dir: PathBuf,
) -> Result<BlacklineDir, Error> {
    let failed = |what: &str, error: std::io::Error| {
        Error::refused(
            ErrorCode::BlacklineFailed,
            format!("the blackline's copies could not be written ({what}): {error}"),
        )
    };
    if let Some(parent) = dir.parent() {
        crate::write::create_dir_all(parent).map_err(|error| failed("its folder", error))?;
    }
    crate::write::create_private_dir(&dir).map_err(|error| failed("its folder", error))?;
    let made = BlacklineDir { path: dir };
    let mut old = Vec::new();
    matter.read_file(from, path, &mut old)?;
    let new = match to {
        CompareSide::Checkpoint(to) => {
            let mut new = Vec::new();
            matter.read_file(to, path, &mut new)?;
            new
        }
        CompareSide::Folder => folder_file(matter.folder(), path)?,
    };
    for (name, bytes) in [(OLD, &old), (NEW, &new)] {
        crate::write::create_new_file(&made.path.join(name), bytes)
            .map_err(|error| failed(name, error))?;
    }
    Ok(made)
}

/// The bytes of `path` in the matter's folder as it is now. Anything but a file there (nothing,
/// a folder, a link) is `Error::UnknownFile`.
fn folder_file(folder: &Path, path: &MatterPath) -> Result<Vec<u8>, Error> {
    let unknown = || Error::from(clauseo_matters::Error::UnknownFile { path: path.clone() });
    let file = folder.join(path.as_str());
    match std::fs::symlink_metadata(&file) {
        Ok(meta) if meta.is_file() => std::fs::read(&file).map_err(|error| {
            Error::refused(
                ErrorCode::BlacklineFailed,
                format!("the file in the folder could not be read: {error}"),
            )
        }),
        Ok(_) => Err(unknown()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(unknown()),
        Err(error) => Err(Error::refused(
            ErrorCode::BlacklineFailed,
            format!("the file in the folder could not be read: {error}"),
        )),
    }
}
