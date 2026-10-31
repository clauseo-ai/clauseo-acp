//! Reading in a matter folder without downloading cloud-only files (research/39 §3 and §6).
//!
//! A Mac app started through Launch Services runs with the process policy for dataless files
//! on, so any read of a cloud-only file would download it. [`NoDownload`] turns that off for the
//! calling thread while it lives, and puts back what the thread had when dropped: the crate reads
//! on the caller's thread, which can be a shared pool thread, and the rest of the app (Word
//! preview, Quick Look, the lawyer's own opens) may need downloads. Never process-wide. On
//! other OSes it changes nothing.
//!
//! Every function that reads a file's content in the matter folder, and the walk's listing,
//! takes a `&NoDownload`, so such a read without a guard does not compile. The guard is taken in
//! three places, and every public entry point that reads the folder goes through one of them:
//!
//! - the checkpoint walk (`checkpoint/walk_folder.rs`): `Matter::create` and
//!   `Matter::import` (the root checkpoint), `Matter::before_turn` and `Matter::after_turn`,
//!   `Matter::undo` (its two checkpoints), `Matter::restore` (its walk and its checkpoint),
//!   `Matter::preview_restore` and `Matter::compare` with the folder (the hash-only walk);
//! - `undo::edited_since`: `Matter::edited_since` (hashes of the step's targets);
//! - [`crate::scan`]: its listings (it reads no content).
//!
//! The others read no file in the folder but `matter.json`: `Matter::open`, `locate`,
//! `relink`, `changes_between`, `files_at`, `read_file`, `compare` of two checkpoints,
//! `id`, `folder`, `timeline` and `last_checkpoint`.
//!
//! The policy test in `tests/policy.rs` runs each guarded entry point on macOS and checks that
//! every read and listing ran with downloads off.
//!
//! Outside the guard: `.clauseo/matter.json` (ours: downloading it is right); the history
//! reads of open, compare and write-back; and the listings that undo and restore make to match
//! names byte for byte (`writeback::Names`), which run in their plan, outside any walk (a listing
//! never downloads a file's content, research/39 §3; a folder a File Provider has not
//! downloaded may be, QR9.1). `edited_since` makes the same listings under its guard and counts
//! a file behind a folder that does not list as unread. Inside the guard but not in the folder:
//! the check of whether a blob is already stored (`durable::present`, during the walk's
//! reads) reads history objects; with the matters root on a File Provider, which the host does
//! not do, a refused read there would count the folder's file as cloud-only.
//!
//! With the guard on, a read of a file that went cloud-only after its stat fails with
//! `EDEADLK` instead of downloading it, and so does listing a folder that is not downloaded.
//! [`not_downloaded`] names that error; the callers treat it as the stat-time cloud-only case and
//! never read again.

use std::io;
use std::marker::PhantomData;
use std::path::Path;

use crate::PlaceholderProbe;

#[cfg(test)]
pub(crate) mod fault;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(test)]
mod tests;

/// While alive, reads on this thread do not download a cloud-only file: on macOS the thread's
/// policy for dataless files is off, so such a read fails with `EDEADLK`. Dropping it puts
/// back the policy the thread had, on an error return and while unwinding too. Not `Send`:
/// the policy belongs to the thread that set it.
///
/// It also carries the settings' probe. Every read that needs the guard also needs the probe
/// (the walk classifies what it lists; a read checks the metadata of the file it opened before
/// reading it), so one argument proves both: no read can run without the policy off, and none
/// can skip the open-file check.
#[must_use = "the policy is put back when the guard is dropped"]
pub(crate) struct NoDownload<'a> {
    probe: &'a dyn PlaceholderProbe,
    /// The thread's policy before, to put back; `None` when setting it failed.
    #[cfg(target_os = "macos")]
    saved: Option<std::ffi::c_int>,
    /// Neither `Send` nor `Sync`.
    _thread: PhantomData<*const ()>,
}

impl<'a> NoDownload<'a> {
    /// Turns downloads off for this thread (macOS) until the guard drops. If the system refuses
    /// (the App Sandbox may), logs a warning and reads go on as they would without it.
    pub(crate) fn begin(probe: &'a dyn PlaceholderProbe) -> Self {
        Self {
            probe,
            #[cfg(target_os = "macos")]
            saved: macos::turn_off(),
            _thread: PhantomData,
        }
    }

    /// Whether the regular file at `path` with `metadata` is cloud-only, by the settings' probe.
    pub(crate) fn is_cloud_only(&self, path: &Path, metadata: &std::fs::Metadata) -> bool {
        self.probe.is_cloud_only(path, metadata)
    }

    /// The settings' probe.
    pub(crate) fn probe(&self) -> &'a dyn PlaceholderProbe {
        self.probe
    }
}

impl Drop for NoDownload<'_> {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(saved) = self.saved {
            macos::put_back(saved);
        }
    }
}

impl std::fmt::Debug for NoDownload<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NoDownload")
    }
}

/// Whether `err` says the data of a file or folder is not on this computer: `EDEADLK`, matched
/// by its name (`io::ErrorKind::Deadlock`), never by its number (11 is `EAGAIN` on Linux).
pub(crate) fn not_downloaded(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::Deadlock
}

/// The error a read gives for a file the probe finds cloud-only once it is open: the same kind
/// as the read under the guard would give, so one rule handles both.
pub(crate) fn not_downloaded_error() -> io::Error {
    io::Error::new(io::ErrorKind::Deadlock, "the file is not downloaded")
}
