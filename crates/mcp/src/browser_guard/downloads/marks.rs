//! The OS's mark on a file from the internet, which makes the OS check the file when the lawyer
//! opens it: on Windows the Mark of the Web, a `Zone.Identifier` stream (`SmartScreen`, Defender,
//! Office's Protected View); on macOS the `com.apple.quarantine` attribute (Gatekeeper). Linux
//! has neither, so there the guard writes none.
//!
//! Chrome marks every download itself, Safe Browsing on or off: `DownloadItemImpl` always asks
//! for the rename with annotation (`download_item_impl.cc`, "even if it already has the right
//! name, we need the annotation"), and `ChromeDownloadManagerDelegate::ConnectToQuarantineService`
//! binds the quarantine service with no pref, policy or Safe Browsing check. On Windows that
//! runs Attachment Services (`IAttachmentExecute::Save`) or writes `[ZoneTransfer] ZoneId=3`
//! itself (`quarantine_win.cc`); on macOS it sets the quarantine properties
//! (`quarantine_mac.mm`). It marks the file after renaming it to its final name, so the mover
//! waits for the mark ([`super::MARK_WAIT`]) before it moves a file.
//!
//! [`ensure`] then makes sure of the mark in the staging folder, out of the agent's reach, and
//! the move keeps it: a rename within one volume keeps the file's streams on Windows, and the
//! Unix hard link is the same inode with its attributes. A download that must be copied instead
//! (the downloads folder on another drive, or a file system without hard links:
//! `write/copy.rs`) gets its mark from [`mark_copy`] on the new, still empty copy, which is
//! checked by reading it back: so the mark is tested on each destination's own drive, file by
//! file, and a drive that cannot keep it (FAT, exFAT, a share without streams or attributes) gets
//! no copy at all.

use std::io;
use std::path::Path;

/// Whether this OS has a mark the guard keeps: Windows and macOS.
const HAS_MARKS: bool = cfg!(any(windows, target_os = "macos"));

/// The scratch file [`probe`] marks in the staging folder; the mover never moves it.
pub(crate) const PROBE: &str = ".clauseo-mark-probe";

/// Whether `file` carries a mark already, Chrome's or any other; true where the guard writes
/// none (Linux).
pub(crate) fn marked(file: &Path) -> bool {
    os::marked(file)
}

/// Makes sure `file`, in the staging folder, has its OS's mark. On Windows a `Zone.Identifier`
/// whose `ZoneId` is 1 (intranet), 2 (trusted), 3 (internet) or 4 (restricted) is kept, as Edge
/// keeps it: a page cannot pick its zone (Attachment Services maps it from the source URL), so 1
/// and 2 come only from the firm's own zone settings. No stream, `ZoneId=0` (this computer), or
/// one that does not read becomes `ZoneId=3`. On macOS a `com.apple.quarantine` already there is
/// kept as it is.
pub(crate) fn ensure(file: &Path) -> io::Result<()> {
    if !HAS_MARKS {
        return Ok(());
    }
    os::ensure(file)
}

/// Marks `copy`, a new empty file another drive holds, as `source` (the staging file, marked
/// by [`ensure`]) is marked, then reads the mark back: on Windows the source's
/// `Zone.Identifier` text when its zone is kept (with Chrome's `HostUrl`), else `ZoneId=3`;
/// on macOS the source's `com.apple.quarantine` value, else a new one. Both are written through
/// the copy's open handle, never its path. `Err` when the drive does not keep it. Nothing on
/// Linux.
pub(crate) fn mark_copy(source: &std::fs::File, copy: &std::fs::File) -> io::Result<()> {
    if !HAS_MARKS {
        return Ok(());
    }
    os::mark_copy(source, copy)
}

/// Whether `error`, from [`mark_copy`], says the drive cannot keep the mark at all, so the file
/// goes to the fallback folder: a mark that did not read back (no OS error), or the OS's
/// "not supported" or "not permitted" for the attribute or stream (Unix `ENOTSUP`,
/// `EOPNOTSUPP`, `EPERM`, `ENOSYS`; Windows `ERROR_INVALID_FUNCTION`, `ERROR_NOT_SUPPORTED`,
/// `ERROR_INVALID_PARAMETER`, `ERROR_INVALID_NAME`: FAT and exFAT refuse a stream's name).
/// Any other error (a sharing violation or a denial while a scanner holds the new file,
/// `EBUSY`, `EINTR`, a full disk) is not the drive's: the copy is tried again.
pub(crate) fn cannot_keep(error: &io::Error) -> bool {
    let Some(code) = error.raw_os_error() else {
        return true;
    };
    #[cfg(unix)]
    {
        use nix::libc;
        [libc::ENOTSUP, libc::EOPNOTSUPP, libc::EPERM, libc::ENOSYS].contains(&code)
    }
    #[cfg(windows)]
    {
        /// `ERROR_INVALID_FUNCTION`, `ERROR_NOT_SUPPORTED`, `ERROR_INVALID_PARAMETER` and
        /// `ERROR_INVALID_NAME`.
        const CANNOT: [i32; 4] = [1, 50, 87, 123];
        CANNOT.contains(&code)
    }
}

/// Whether the staging folder's drive keeps a mark: writes [`PROBE`], marks it, reads the mark
/// back and removes it. Always `Ok` on Linux.
pub(crate) fn probe(staging: &Path) -> io::Result<()> {
    if !HAS_MARKS {
        return Ok(());
    }
    os::probe(staging)
}

/// The `ZoneId` a `Zone.Identifier` stream's text names, if it names one that reads.
#[cfg(any(windows, test))]
pub(crate) fn zone_of(text: &str) -> Option<u32> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("ZoneId="))
        .and_then(|zone| zone.trim().parse().ok())
}

/// Whether a stream naming `zone` is kept as it is: zones 1 to 4.
#[cfg(any(windows, test))]
pub(crate) fn zone_kept(zone: Option<u32>) -> bool {
    matches!(zone, Some(1..=4))
}

/// The `com.apple.quarantine` value the guard writes, in the attribute's format
/// `<flags>;<hex seconds since 1970>;<agent>;<event id>`: flags `0081`, the usual value for a
/// downloaded file, and no event id (no entry in the quarantine events database).
#[cfg(any(target_os = "macos", test))]
pub(crate) fn quarantine_value(seconds: u64) -> String {
    format!("0081;{seconds:x};Clauseo;")
}

/// The mark as it is on disk, for the tests: the stream's text or the attribute's value.
#[cfg(test)]
pub(crate) fn read(file: &Path) -> Option<String> {
    os::read(file)
}

/// [`PROBE`] in `staging`, marked with `ensure` and checked with `marked`, then removed.
#[cfg(any(windows, target_os = "macos"))]
fn probe_with(staging: &Path, check: impl Fn(&Path) -> bool) -> io::Result<()> {
    let probe = staging.join(PROBE);
    crate::write::write(&probe, "")?;
    let result = ensure(&probe).and_then(|()| {
        if check(&probe) {
            Ok(())
        } else {
            Err(io::Error::other("the mark did not read back"))
        }
    });
    drop(crate::write::remove_file(&probe));
    result
}

#[cfg(windows)]
mod os {
    use std::ffi::OsString;
    use std::io;
    use std::path::{Path, PathBuf};

    use crate::write;

    /// The Mark of the Web the guard writes: the Internet zone. No `HostUrl`, which the guard
    /// does not know.
    pub(crate) const ZONE_MARK: &str = "[ZoneTransfer]\r\nZoneId=3\r\n";

    /// `file`'s `Zone.Identifier` stream.
    fn stream(file: &Path) -> PathBuf {
        let mut name = OsString::from(file.as_os_str());
        name.push(":Zone.Identifier");
        PathBuf::from(name)
    }

    pub(super) fn marked(file: &Path) -> bool {
        std::fs::metadata(stream(file)).is_ok()
    }

    /// The file is in staging, which Chrome made and the agent cannot reach, and the sweep saw
    /// it as a regular file, so the stream's open follows no planted link.
    pub(super) fn ensure(file: &Path) -> io::Result<()> {
        let zone = std::fs::read_to_string(stream(file))
            .ok()
            .and_then(|text| super::zone_of(&text));
        if super::zone_kept(zone) {
            return Ok(());
        }
        write::write(&stream(file), ZONE_MARK)
    }

    /// The stream's name, relative to a file's handle.
    const STREAM: &str = ":Zone.Identifier";

    pub(super) fn mark_copy(source: &std::fs::File, copy: &std::fs::File) -> io::Result<()> {
        let read = |file: &std::fs::File| -> io::Result<String> {
            let mut text = String::new();
            io::Read::read_to_string(&mut write::open_stream(file, STREAM, false)?, &mut text)?;
            Ok(text)
        };
        let text = read(source)
            .ok()
            .filter(|text| super::zone_kept(super::zone_of(text)))
            .unwrap_or_else(|| ZONE_MARK.to_owned());
        let mut stream = write::open_stream(copy, STREAM, true)?;
        io::Write::write_all(&mut stream, text.as_bytes())?;
        drop(stream);
        if super::zone_of(&read(copy)?) == super::zone_of(&text) {
            Ok(())
        } else {
            Err(io::Error::other("the mark did not read back"))
        }
    }

    pub(super) fn probe(staging: &Path) -> io::Result<()> {
        super::probe_with(staging, |file| {
            std::fs::read_to_string(stream(file)).is_ok_and(|text| super::zone_of(&text) == Some(3))
        })
    }

    #[cfg(test)]
    pub(super) fn read(file: &Path) -> Option<String> {
        std::fs::read_to_string(stream(file)).ok()
    }

    /// The stream's path, for the tests' fixtures.
    #[cfg(test)]
    pub(crate) fn stream_of(file: &Path) -> PathBuf {
        stream(file)
    }
}

#[cfg(target_os = "macos")]
mod os {
    use std::ffi::CStr;
    use std::io;
    use std::path::Path;

    use crate::write;

    /// The attribute's name.
    const NAME: &CStr = c"com.apple.quarantine";

    pub(super) fn marked(file: &Path) -> bool {
        write::has_xattr(file, NAME)
    }

    /// The time stamp is the file's last change, when Chrome finished writing it: the download's
    /// time, read without a clock.
    pub(super) fn ensure(file: &Path) -> io::Result<()> {
        if marked(file) {
            return Ok(());
        }
        let seconds = std::fs::metadata(file)?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        write::set_xattr(file, NAME, super::quarantine_value(seconds).as_bytes())
    }

    pub(super) fn mark_copy(source: &std::fs::File, copy: &std::fs::File) -> io::Result<()> {
        let found = write::fget_xattr(source, NAME).filter(|value| !value.is_empty());
        let value = if let Some(value) = found {
            value
        } else {
            let seconds = source
                .metadata()?
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            super::quarantine_value(seconds).into_bytes()
        };
        write::fset_xattr(copy, NAME, &value)?;
        if write::fget_xattr(copy, NAME).as_deref() == Some(value.as_slice()) {
            Ok(())
        } else {
            Err(io::Error::other("the mark did not read back"))
        }
    }

    pub(super) fn probe(staging: &Path) -> io::Result<()> {
        super::probe_with(staging, marked)
    }

    #[cfg(test)]
    pub(super) fn read(file: &Path) -> Option<String> {
        String::from_utf8(write::read_xattr(file, NAME)?).ok()
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod os {
    use std::io;
    use std::path::Path;

    pub(super) fn marked(_file: &Path) -> bool {
        true
    }

    /// Never called: `HAS_MARKS` is false here.
    pub(super) fn ensure(_file: &Path) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    /// Never called: `HAS_MARKS` is false here.
    pub(super) fn mark_copy(_source: &std::fs::File, _copy: &std::fs::File) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    /// Never called: `HAS_MARKS` is false here.
    pub(super) fn probe(_staging: &Path) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    #[cfg(test)]
    pub(super) fn read(_file: &Path) -> Option<String> {
        None
    }
}

#[cfg(all(windows, test))]
pub(crate) use os::{ZONE_MARK, stream_of};
