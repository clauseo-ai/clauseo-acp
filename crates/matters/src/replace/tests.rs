//! The mapping of a failed read in `read_local`: a file another program holds with no sharing
//! is `matter.file_locked`. A real sharing violation cannot be made off Windows, so the error
//! codes are tested as numbers.

use std::io;
use std::path::Path;

use clauseo_protocol::MatterPath;

use super::read_failed;
use crate::Error;
use crate::locks::held_by_read_code;

fn path() -> MatterPath {
    MatterPath::new("Deals/SPA.docx").expect("a valid path")
}

#[test]
fn a_sharing_or_lock_violation_is_a_held_file() {
    for code in [32, 33] {
        assert!(held_by_read_code(Some(code)), "{code}");
    }
    for other in [None, Some(2), Some(5), Some(13)] {
        assert!(!held_by_read_code(other), "{other:?}");
    }
}

#[test]
fn a_read_failed_on_a_held_file_names_the_file_as_locked() {
    let err = io::Error::from_raw_os_error(32);
    let held = held_by_read_code(err.raw_os_error());
    let mapped = read_failed(Path::new("/m/Deals/SPA.docx"), &path(), "read", err, held);
    assert!(
        matches!(&mapped, Error::FileLocked { paths } if paths == &[path()]),
        "{mapped:?}"
    );
}

#[test]
fn another_read_failure_stays_an_io_error() {
    let err = io::Error::from(io::ErrorKind::PermissionDenied);
    let held = held_by_read_code(err.raw_os_error());
    let mapped = read_failed(Path::new("/m/Deals/SPA.docx"), &path(), "read", err, held);
    assert!(matches!(mapped, Error::Io { .. }), "{mapped:?}");
}

/// On Windows the function `read_local` calls agrees with the numbers.
#[cfg(windows)]
#[test]
fn windows_reads_a_sharing_violation_as_held() {
    assert!(crate::locks::held_at_read(&io::Error::from_raw_os_error(
        32
    )));
    assert!(crate::locks::held_at_read(&io::Error::from_raw_os_error(
        33
    )));
    assert!(!crate::locks::held_at_read(&io::Error::from_raw_os_error(
        5
    )));
}
