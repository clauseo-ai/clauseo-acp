//! The matter thread's part of a Word blackline (`MatterJob::WriteSides`): it copies the two
//! sides of one document into a fresh folder outside the matter, and nothing else. stetdoc runs
//! later, off the actor (`actor/query/blackline.rs`).
//!
//! It answers: where the copies go (`<dir>/old.docx` and `<dir>/new.docx`, in the one folder
//! `dir`, made here and private to the user on Unix); what each side is (`Matter::read_file` for
//! a version; for the folder now, a copy of the file itself, never through a link, both streamed);
//! what a missing, over-cap or cloud-only file answers (`Error::UnknownFile`, as `read_file`
//! answers a missing one); and how the folder
//! goes ([`BlacklineDir`], whose drop removes it on every path).

use std::io::{self, Read as _, Write};
use std::path::{Path, PathBuf};

use clauseo_matters::{CompareSide, Matter, PlaceholderProbe, Settings};
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
            tracing::warn!(
                kind = ?error.kind(),
                "a Word blackline's temporary folder could not be removed"
            );
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
        MatterReply::SidesWritten(open(settings, slot).and_then(|matter| {
            write_sides(matter, settings, self.from, self.to, &self.path, self.dir)
        }))
    }
}

/// Makes `dir` (its parent too, when missing) and streams `from`'s copy of `path` there as
/// [`OLD`] and `to`'s as [`NEW`], neither held in memory whole. A file the version or the
/// folder lacks is `Error::UnknownFile` (`command.invalid`), and so is a side `CompareVersions`
/// would not see as content: past the size cap (`Limits::effective_cap`), or, in the folder, a
/// cloud-only placeholder (the settings' probe), whose read would download it. A copy that can't
/// be written is `blackline.failed`. On any error the folder is gone before this returns.
fn write_sides(
    matter: &Matter,
    settings: &Settings,
    from: CheckpointId,
    to: CompareSide,
    path: &MatterPath,
    dir: PathBuf,
) -> Result<BlacklineDir, Error> {
    if let Some(parent) = dir.parent() {
        crate::write::create_dir_all(parent).map_err(|error| failed("its folder", &error))?;
    }
    crate::write::create_private_dir(&dir).map_err(|error| failed("its folder", &error))?;
    let made = BlacklineDir { path: dir };
    let cap = settings.limits.effective_cap();
    version_side(matter, from, path, cap, &made.path.join(OLD))?;
    match to {
        CompareSide::Checkpoint(to) => version_side(matter, to, path, cap, &made.path.join(NEW))?,
        CompareSide::Folder => folder_side(
            &*settings.probe,
            cap,
            &matter.folder().join(path.as_str()),
            path,
            &made.path.join(NEW),
        )?,
    }
    Ok(made)
}

/// `blackline.failed` for a copy that could not be made at `what`, by the OS's error kind
/// alone: never its text or a path (ADR 0047).
fn failed(what: &str, error: &io::Error) -> Error {
    Error::refused(
        ErrorCode::BlacklineFailed,
        format!(
            "the blackline's copies could not be written ({what}): {:?}",
            error.kind()
        ),
    )
}

/// The refusal of `path`, a side `CompareVersions` does not see as content.
fn unknown(path: &MatterPath) -> Error {
    Error::from(clauseo_matters::Error::UnknownFile { path: path.clone() })
}

/// Streams `path` as version `checkpoint` holds it into the new file `dest`, at most `cap`
/// bytes: past it the side is refused as unknown.
fn version_side(
    matter: &Matter,
    checkpoint: CheckpointId,
    path: &MatterPath,
    cap: u64,
    dest: &Path,
) -> Result<(), Error> {
    let file = crate::write::create_new_file(dest).map_err(|error| failed("a copy", &error))?;
    let mut out = Capped::new(file, cap);
    match matter.read_file(checkpoint, path, &mut out) {
        Ok(_) => Ok(()),
        Err(_) if out.over => Err(unknown(path)),
        Err(error) => Err(error.into()),
    }
}

/// Streams the file `file` (`path` in the matter's folder now) into the new file `dest`.
/// Anything but a regular file there (nothing, a folder, a link) is unknown, as are a cloud-only
/// placeholder by `probe`, checked on the opened file before any read, and a file of more than
/// `cap` bytes, checked before the copy and again by what it read.
fn folder_side(
    probe: &dyn PlaceholderProbe,
    cap: u64,
    file: &Path,
    path: &MatterPath,
    dest: &Path,
) -> Result<(), Error> {
    let read_failed = |error: &io::Error| failed("the file in the folder", error);
    match std::fs::symlink_metadata(file) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Err(unknown(path)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(unknown(path)),
        Err(error) => return Err(read_failed(&error)),
    }
    let opened = std::fs::File::open(file).map_err(|error| read_failed(&error))?;
    let meta = opened.metadata().map_err(|error| read_failed(&error))?;
    if !meta.is_file() || probe.is_cloud_only(file, &meta) || meta.len() > cap {
        return Err(unknown(path));
    }
    let mut out = crate::write::create_new_file(dest).map_err(|error| failed("a copy", &error))?;
    let copied = io::copy(&mut opened.take(cap.saturating_add(1)), &mut out)
        .map_err(|error| read_failed(&error))?;
    if copied > cap {
        return Err(unknown(path));
    }
    Ok(())
}

/// A writer that takes at most `left` bytes: a write past them fails, and `over` says why.
struct Capped<W> {
    inner: W,
    left: u64,
    over: bool,
}

impl<W> Capped<W> {
    fn new(inner: W, cap: u64) -> Self {
        Self {
            inner,
            left: cap,
            over: false,
        }
    }
}

impl<W: Write> Write for Capped<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        if len > self.left {
            self.over = true;
            return Err(io::Error::other("past the size cap"));
        }
        let written = self.inner.write(buf)?;
        self.left -= u64::try_from(written).unwrap_or(u64::MAX).min(self.left);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests;
