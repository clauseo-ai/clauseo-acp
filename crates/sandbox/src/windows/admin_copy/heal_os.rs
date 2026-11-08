//! The heal's Windows side (`heal.rs`): the refresh's heal of the group's deny, as SYSTEM
//! ([`heal_deny`]), and the lawyer's side's wait and clock.
#![expect(unsafe_code, reason = "FFI to GetFileInformationByHandle")]

use std::io;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_READ_ATTRIBUTES, GetFileInformationByHandle, READ_CONTROL,
    WRITE_DAC, WRITE_OWNER,
};

use super::heal::chain_safe;
use super::os::{Handle, lock_handle, security_of};
use super::{AdminCopyFs, Owed, admin_dir, admin_exe, owed};
use crate::windows::sids::Sid;

/// `FILE_ATTRIBUTE_DIRECTORY`.
const DIRECTORY: u32 = 0x10;
/// `FILE_ATTRIBUTE_REPARSE_POINT`.
const REPARSE_POINT: u32 = 0x400;

/// The refresh's heal, as SYSTEM: nothing unless every level from the volume root to the copy
/// under `fs`'s `%ProgramFiles%` passes the check's chain rule ([`chain_safe`]); then
/// [`heal_at`] on the admin folder and the copy. Whether anything was locked again.
pub(crate) fn heal_deny(fs: &dyn AdminCopyFs, group: &Sid) -> io::Result<bool> {
    let program_files = fs.program_files()?;
    if !chain_safe(fs, &program_files) {
        return Ok(false);
    }
    heal_at(
        &admin_dir(&program_files),
        &admin_exe(&program_files),
        group,
    )
}

/// The folder `dir` and the copy `exe`, each opened once as itself (never through a link at
/// it), refused unless a plain folder and a plain file; their descriptors read through those
/// handles, and when [`owed`] says either lacks only `group`'s deny, each that lacks it given
/// the locked owner and DACL with the deny through the same handle, read back exactly
/// ([`lock_handle`], which also refuses a file with a second name). Anything else is left.
pub(crate) fn heal_at(dir: &Path, exe: &Path, group: &Sid) -> io::Result<bool> {
    let access = READ_CONTROL.0 | WRITE_DAC.0 | WRITE_OWNER.0 | FILE_READ_ATTRIBUTES.0;
    let folder = Handle::open(dir, access)?;
    let copy = Handle::open(exe, access)?;
    if !plain(folder.0, true)? || !plain(copy.0, false)? {
        return Ok(false);
    }
    let Owed::Lacking {
        folder: folder_lacks,
        copy: copy_lacks,
    } = owed(&security_of(folder.0)?, &security_of(copy.0)?, group)
    else {
        return Ok(false);
    };
    if folder_lacks {
        lock_handle(folder.0, true, Some(group))?;
    }
    if copy_lacks {
        lock_handle(copy.0, false, Some(group))?;
    }
    Ok(true)
}

/// Whether what `handle` is open on is no reparse point, and a folder exactly when `folder`.
fn plain(handle: HANDLE, folder: bool) -> io::Result<bool> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open for the call; `info` is a live, writable struct.
    unsafe { GetFileInformationByHandle(handle, &raw mut info) }
        .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF))?;
    let attributes = info.dwFileAttributes;
    Ok(attributes & REPARSE_POINT == 0 && (attributes & DIRECTORY != 0) == folder)
}

/// Sleeps `pause`: the heal's wait, off the actor's thread.
#[expect(
    clippy::disallowed_methods,
    reason = "the heal's bounded wait, which the tests' fake replaces (AdminCopyFs::pause)"
)]
pub(super) fn wait(pause: Duration) {
    std::thread::sleep(pause);
}

/// The time since this process first asked: the heal's back-off clock.
#[expect(
    clippy::disallowed_methods,
    reason = "the heal's back-off clock, which the tests' fake replaces (AdminCopyFs::clock)"
)]
pub(super) fn clock() -> Duration {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed()
}
