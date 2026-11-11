//! A local FAT or exFAT volume has no file IDs: Windows' `FileIdInfo` query fails there with
//! `ERROR_INVALID_PARAMETER`. [`no_file_ids`] decides, by pure rules tested on every system,
//! when that failure means "this object has no file ID" rather than "its identity can't be
//! read", and [`file_id_failure`] carries the answer as an [`io::Error`] that
//! [`is_no_file_ids`] recognises.
//!
//! Why such an object is not the data folder: the data fence compares identities only when the
//! data folder's own identity was captured (`agent/data_fence/identity.rs`), so the data
//! folder's volume answers `FileIdInfo`; an object on a local FAT or exFAT volume is on another
//! volume. Hard links stay within one volume, and FAT and exFAT hold no reparse points, so no
//! junction or symbolic link there can stand for the data folder; the walk has already followed
//! every link on the way (`resolve::walk`). A network object never qualifies: a loopback share
//! (`\\localhost\C$\…`) is a second name the text checks can't map back, and a server reports
//! its own disk's file system. If the data folder itself is on exFAT, its identity is never
//! captured and the fence compares names only, as before.
//!
//! Only Windows asks `FileIdInfo`, so off Windows (tests aside) no error carries the marker and
//! [`is_no_file_ids`] is always false.

#[cfg(any(windows, test))]
use std::fmt;
use std::io;

/// `ERROR_INVALID_PARAMETER` (`winerror.h`): `FileIdInfo`'s answer on FAT and exFAT.
#[cfg(any(windows, test))]
pub(crate) const INVALID_PARAMETER: i32 = 87;

/// The file systems with no file IDs, by the exact names Windows reports (any case).
#[cfg(any(windows, test))]
const NO_ID_FILE_SYSTEMS: [&str; 3] = ["FAT", "FAT32", "exFAT"];

/// Whether an object has no file ID, rather than an identity that can't be read: only when the
/// `FileIdInfo` query failed with [`INVALID_PARAMETER`] (`file_id_error`, `None` when it worked),
/// the volume's file system is named FAT, FAT32 or exFAT (`fs_name`, `None` when it can't be
/// read), and the object is not on the network (`remote`). Anything else stays fail-closed.
#[cfg(any(windows, test))]
pub(crate) fn no_file_ids(file_id_error: Option<i32>, fs_name: Option<&str>, remote: bool) -> bool {
    file_id_error == Some(INVALID_PARAMETER)
        && !remote
        && fs_name.is_some_and(|name| {
            NO_ID_FILE_SYSTEMS
                .iter()
                .any(|known| name.eq_ignore_ascii_case(known))
        })
}

/// The marker inside the error [`file_id_failure`] gives for an object with no file ID.
#[cfg(any(windows, test))]
#[derive(Debug)]
struct NoFileIds;

#[cfg(any(windows, test))]
impl fmt::Display for NoFileIds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("this volume has no file IDs")
    }
}

#[cfg(any(windows, test))]
impl std::error::Error for NoFileIds {}

/// The error for a failed `FileIdInfo` query (`code`): one [`is_no_file_ids`] recognises when
/// [`no_file_ids`] holds, else the system error `code` itself.
#[cfg(any(windows, test))]
pub(crate) fn file_id_failure(code: i32, fs_name: Option<&str>, remote: bool) -> io::Error {
    if no_file_ids(Some(code), fs_name, remote) {
        io::Error::new(io::ErrorKind::Unsupported, NoFileIds)
    } else {
        io::Error::from_raw_os_error(code)
    }
}

/// Whether `error` says the object has no file ID ([`file_id_failure`]).
#[cfg(any(windows, test))]
pub(crate) fn is_no_file_ids(error: &io::Error) -> bool {
    matches!(error.get_ref(), Some(inner) if inner.is::<NoFileIds>())
}

/// Off Windows no error says so: only Windows asks `FileIdInfo`.
#[cfg(not(any(windows, test)))]
pub(crate) fn is_no_file_ids(_error: &io::Error) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_local_fat_or_exfat_volume_answering_87_has_no_file_ids() {
        for name in ["FAT", "FAT32", "exFAT", "EXFAT", "fat32", "fat"] {
            assert!(no_file_ids(Some(87), Some(name), false), "{name}");
            assert!(!no_file_ids(Some(87), Some(name), true), "{name} remote");
        }
        for name in ["NTFS", "ReFS", "", "exFAT ", "FAT16", "vfat", "exFAT64"] {
            assert!(!no_file_ids(Some(87), Some(name), false), "{name}");
        }
        assert!(!no_file_ids(Some(87), None, false));
        for code in [5, 1, 50, 2, 123] {
            assert!(!no_file_ids(Some(code), Some("exFAT"), false), "{code}");
        }
        assert!(!no_file_ids(None, Some("exFAT"), false));
    }

    #[test]
    fn the_failure_carries_the_answer() {
        assert!(is_no_file_ids(&file_id_failure(87, Some("exFAT"), false)));
        let remote = file_id_failure(87, Some("exFAT"), true);
        assert!(!is_no_file_ids(&remote));
        assert_eq!(remote.raw_os_error(), Some(87));
        assert!(!is_no_file_ids(&file_id_failure(87, Some("NTFS"), false)));
        assert!(!is_no_file_ids(&file_id_failure(5, Some("FAT"), false)));
        assert!(!is_no_file_ids(&io::ErrorKind::Unsupported.into()));
        assert!(!is_no_file_ids(&io::Error::other(
            "this volume has no file IDs"
        )));
    }
}
