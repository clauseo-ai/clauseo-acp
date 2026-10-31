use std::io;
use std::path::Path;

use super::{DARWIN_NO_FULL_FLUSH, settle_full_flush};

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
