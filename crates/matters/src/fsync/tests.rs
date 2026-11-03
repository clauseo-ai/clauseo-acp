use std::io;
use std::path::Path;

use super::{DARWIN_NO_FULL_FLUSH, settle_full_flush};

/// Windows: `FlushFileBuffers` needs write access, which is why `write::open_to_sync` opens
/// for writing there; a file opened read-only is refused, not passed as synced.
#[cfg(windows)]
#[test]
fn syncing_a_file_opened_read_only_is_refused() {
    let dir = crate::test_dir::TestDir::new("fsync-read-only");
    let file = dir.path().join("a.txt");
    drop(crate::write::create_new(&file).unwrap());
    let read_only = std::fs::File::open(&file).unwrap();
    let err = super::sync_file(&read_only).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "{err}");
    drop(read_only);
    super::sync_file(&crate::write::open_to_sync(&file).unwrap()).unwrap();
}

#[test]
fn a_file_system_without_full_flush_settles_for_the_folder_syncs() {
    let dir = Path::new("/matter");
    for code in DARWIN_NO_FULL_FLUSH {
        let err = io::Error::from_raw_os_error(code);
        assert!(settle_full_flush(dir, Err(err)).is_ok(), "errno {code}");
    }
    assert!(settle_full_flush(dir, Ok(())).is_ok());
    // EIO, ENOSPC, EBADF, and an error with no errno, are failures.
    for code in [5, 28, 9] {
        let err = io::Error::from_raw_os_error(code);
        assert!(settle_full_flush(dir, Err(err)).is_err(), "errno {code}");
    }
    assert!(settle_full_flush(dir, Err(io::Error::other("disk"))).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn the_fixed_numbers_are_darwins() {
    use nix::errno::Errno;
    let codes = [Errno::ENOTSUP, Errno::ENOTTY, Errno::EINVAL].map(|errno| errno as i32);
    assert_eq!(codes, DARWIN_NO_FULL_FLUSH);
}

/// macOS: syncing a folder that is gone fails; it is not passed as synced.
#[cfg(target_os = "macos")]
#[test]
fn syncing_a_folder_that_is_gone_fails() {
    let dir = crate::test_dir::TestDir::new("fsync-gone");
    let gone = dir.path().join("gone");
    let err = super::sync_dir(&gone).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
    let err = super::full_flush_dir(&gone).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
    super::sync_dir(dir.path()).unwrap();
    super::full_flush_dir(dir.path()).unwrap();
}

/// macOS: each sync call reports the OS's refusal instead of passing as done. A pipe is no
/// file on a disk, so `fsync`, `F_BARRIERFSYNC` and `F_FULLFSYNC` all fail on it.
#[cfg(target_os = "macos")]
#[test]
fn a_sync_the_os_refuses_is_an_error() {
    let (reader, _writer) = io::pipe().unwrap();
    let pipe = std::fs::File::from(std::os::fd::OwnedFd::from(reader));
    assert!(super::sync_file(&pipe).is_err());
    assert!(super::barrier(&pipe).is_err());
    assert!(super::full_flush(&pipe).is_err());
}
