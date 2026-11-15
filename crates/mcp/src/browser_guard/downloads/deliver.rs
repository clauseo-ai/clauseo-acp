//! Delivering one finished download: a move into the downloads folder, a copy where no move can
//! be made, and the fallback folder where the downloads folder's drive cannot keep the mark.
//!
//! 1. A move ([`write::Folder::move_new`]), which keeps the mark made in staging.
//! 2. Where the move cannot be made at all ([`write::needs_copy`]: the matter on another drive,
//!    or a file system without hard links), a copy (`write/copy.rs`): a hidden temporary file,
//!    marked before any content ([`marks::mark_copy`], read back, so each destination's drive is
//!    tested, file by file), filled, flushed, then given its name without replacing anything,
//!    with the same names a move takes; then the staging file is deleted.
//! 3. Where the copy's mark does not hold (exFAT, FAT, a share without streams or attributes),
//!    the file is neither refused nor delivered unmarked: it goes to the fallback folder, the
//!    guard's `--fallback-downloads` (`<data>/agent-state/<matter>/downloads`, on the data drive,
//!    outside the agent's reach), marked the same way, and the agent is told its path
//!    ([`super::Saved::record_fallback`]); the host sees that line and shows the lawyer where the
//!    file is. Without a fallback folder, the file stays in staging and the agent is told.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use super::{MOVE_TRIES, Sweeper, free_name, marks, safe_name};
use crate::write::{self, CopyFailed, Mark};

/// The move a delivery tries first (the tests inject one that crosses devices).
pub(crate) type MoveFn = fn(&write::Folder, &Path, &OsStr) -> io::Result<()>;

/// How a delivery moves and marks.
#[derive(Clone, Copy)]
pub(crate) struct How {
    pub(crate) mover: MoveFn,
    pub(crate) mark: Mark,
}

impl Default for How {
    fn default() -> How {
        How {
            mover: write::Folder::move_new,
            mark: marks::mark_copy,
        }
    }
}

/// How many sweeps in a row a copy may fail (a full disk, a share that went away) before the
/// file is held for good: each try copies the whole file.
const COPY_TRIES: u32 = 3;

/// Why a delivery failed.
#[derive(Debug)]
pub(crate) enum Failed {
    /// The folder's drive does not keep the mark: no copy is left there.
    Unmarkable(io::Error),
    /// The copy failed; the staging file is as it was.
    Copy(io::Error),
    /// Anything else (the folder is gone or a link, the file is held open): tried again.
    Retry(io::Error),
    /// Every name the file could take is taken ([`free_name`] up to its last number, or
    /// [`MOVE_TRIES`] names taken in turn between the look and the move): held for good, not
    /// looked for again at every sweep.
    NoFreeName,
}

/// Where a delivered file went, and the error when its staging file could not be deleted.
pub(crate) struct Delivered {
    pub(crate) path: PathBuf,
    pub(crate) source_left: Option<io::Error>,
}

/// Delivers the file `from` into `folder` as `name`, or the first free `<stem> (<n>)<ext>`,
/// never replacing a file: by a move, or by a copy where no move can be made (above). The
/// folder is opened once, first ([`write::Folder::open`], which refuses a link at its name), and
/// every look for a free name, move and copy goes through that handle. When a name is taken by
/// the time of the move or the copy's rename, the next free one is tried, [`MOVE_TRIES`] names at
/// most. `before` runs between each look and its move or rename (the tests put a file there).
pub(crate) fn deliver(
    from: &Path,
    folder: &Path,
    name: &str,
    how: How,
    mut before: impl FnMut(&Path),
) -> Result<Delivered, Failed> {
    let opened = write::Folder::open(folder).map_err(Failed::Retry)?;
    let mut first = 1;
    let mut copy = false;
    for _ in 0..MOVE_TRIES {
        let Some((n, given)) = free_name(&opened, name, first) else {
            return Err(Failed::NoFreeName);
        };
        let target = folder.join(&given);
        before(&target);
        match (how.mover)(&opened, from, OsStr::new(&given)) {
            Ok(()) => {
                return Ok(Delivered {
                    path: target,
                    source_left: None,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => first = n + 1,
            Err(error) if write::needs_copy(&error) => {
                copy = true;
                break;
            }
            Err(error) => return Err(Failed::Retry(error)),
        }
    }
    if !copy {
        return Err(Failed::NoFreeName);
    }
    let mut incoming = opened
        .copy_in(from, how.mark)
        .map_err(|failed| match failed {
            CopyFailed::Mark(error) if marks::cannot_keep(&error) => Failed::Unmarkable(error),
            CopyFailed::Mark(error) | CopyFailed::Io(error) => Failed::Copy(error),
        })?;
    let mut first = 1;
    for _ in 0..MOVE_TRIES {
        let Some((n, given)) = free_name(&opened, name, first) else {
            return Err(Failed::NoFreeName);
        };
        let target = folder.join(&given);
        before(&target);
        match incoming.publish(OsStr::new(&given)) {
            Ok(()) => {
                return Ok(Delivered {
                    path: target,
                    source_left: incoming.remove_source().err(),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => first = n + 1,
            Err(error) => {
                super::super::note(&format!(
                    "a copied download was not given its name, and is not told as saved: {error}"
                ));
                return Err(Failed::Copy(error));
            }
        }
    }
    Err(Failed::NoFreeName)
}

/// Delivers `path`, a finished, marked file in staging Chrome named `name` (above). A file
/// that will never be delivered is held for good (`Sweeper::given_up`): it stays in staging,
/// deleted with the profile, and the agent is told once.
pub(super) fn deliver_one(sweeper: &mut Sweeper, path: &Path, name: &str) {
    let given = safe_name(name);
    let failed = match deliver(path, &sweeper.downloads, &given, sweeper.how, |_| {}) {
        Ok(delivered) => return tell_delivered(sweeper, path, &delivered, false),
        Err(failed) => failed,
    };
    let error = match failed {
        Failed::Retry(error) => return retry_later(sweeper, path, &error),
        Failed::Copy(error) => return copy_failed(sweeper, path, &given, &error),
        Failed::NoFreeName => return no_free_name(sweeper, path, &given),
        Failed::Unmarkable(error) => error,
    };
    super::super::note(&format!(
        "the downloads folder's drive cannot keep the \"from the internet\" mark ({error})"
    ));
    let Some(fallback) = sweeper.fallback.clone() else {
        return hold(
            sweeper,
            path,
            format!(
                "The browser downloaded {given}, but it is not in the matter: the matter's \
                 drive cannot keep the \"downloaded from the internet\" mark, so the file \
                 stayed in Clauseo's browser folder, which is cleared when the browser \
                 closes. Tell your user it was not saved to the matter, and that they can \
                 download it from the page in their own browser."
            ),
        );
    };
    let made = write::make_folder(&fallback).map_err(Failed::Retry);
    match made.and_then(|()| deliver(path, &fallback, &given, sweeper.how, |_| {})) {
        Ok(delivered) => tell_delivered(sweeper, path, &delivered, true),
        Err(Failed::Retry(error)) => retry_later(sweeper, path, &error),
        Err(Failed::Copy(error)) => copy_failed(sweeper, path, &given, &error),
        Err(Failed::NoFreeName) => no_free_name(sweeper, path, &given),
        Err(Failed::Unmarkable(error)) => hold(
            sweeper,
            path,
            format!(
                "The browser downloaded {given}, but it is not saved: neither the matter's \
                 drive nor Clauseo's data folder could keep the \"downloaded from the \
                 internet\" mark ({error}), so the file stayed in Clauseo's browser folder, \
                 which is cleared when the browser closes. Tell your user, and that they \
                 can download it from the page in their own browser."
            ),
        ),
    }
}

/// `path` could not be delivered now, for `error`: left for the next sweep, and noted once.
fn retry_later(sweeper: &mut Sweeper, path: &Path, error: &io::Error) {
    if sweeper.retry_noted.insert(path.to_owned()) {
        super::super::note(&format!(
            "a download waits in staging for the next try: {error}"
        ));
    }
}

/// `path` was delivered: told to the agent, in the downloads folder or, `fallback`, outside
/// the matter. A staging file that could not be deleted is held for good, so it is never
/// delivered twice.
fn tell_delivered(sweeper: &mut Sweeper, path: &Path, delivered: &Delivered, fallback: bool) {
    sweeper.waited.remove(path);
    sweeper.unmarkable.remove(path);
    sweeper.copy_failures.remove(path);
    sweeper.retry_noted.remove(path);
    if let Some(error) = &delivered.source_left {
        super::super::note(&format!(
            "a delivered download stays in staging too, as it could not be deleted: {error}"
        ));
        sweeper.given_up.insert(path.to_owned());
    }
    let (Some(saved), Some(given)) = (&sweeper.saved, delivered.path.file_name()) else {
        return;
    };
    let given = given.to_string_lossy();
    if fallback {
        saved.record_fallback(&given, &delivered.path);
    } else {
        saved.record(&given);
    }
}

/// A copy of `path` failed with `error`: tried again on the next sweep, and held for good at
/// the [`COPY_TRIES`]th failure in a row.
fn copy_failed(sweeper: &mut Sweeper, path: &Path, given: &str, error: &io::Error) {
    let failures = sweeper.copy_failures.entry(path.to_owned()).or_insert(0);
    *failures += 1;
    if *failures < COPY_TRIES {
        return;
    }
    hold(
        sweeper,
        path,
        format!(
            "The browser downloaded {given}, but Clauseo could not copy it into the matter's \
             downloads folder ({error}), so it was not saved. The file stayed in Clauseo's \
             browser folder, which is cleared when the browser closes. Tell your user it was \
             not saved, and that they can download it from the page in their own browser."
        ),
    );
}

/// Every name `path` could take as `given` is taken: held for good, noted and told once.
fn no_free_name(sweeper: &mut Sweeper, path: &Path, given: &str) {
    if !sweeper.given_up.contains(path) {
        super::super::note(&format!(
            "a download is held: every name it could take is taken ({given} and its numbered \
             names up to ({}))",
            super::MAX_NUMBER
        ));
    }
    hold(
        sweeper,
        path,
        format!(
            "The browser downloaded {given}, but it was not saved: the downloads folder already \
             has a file under every name it could take. The file stayed in Clauseo's browser \
             folder, which is cleared when the browser closes. Tell your user, and that they \
             can download it from the page in their own browser."
        ),
    );
}

/// Holds `path` in staging for good and tells the agent `why`, once.
fn hold(sweeper: &mut Sweeper, path: &Path, why: String) {
    if !sweeper.given_up.insert(path.to_owned()) {
        return;
    }
    super::super::note("a download stays in staging for good");
    if let Some(saved) = &sweeper.saved {
        saved.record_unsaved(why);
    }
}
