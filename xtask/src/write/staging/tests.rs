//! Tests of `Staging`: each write stays in bin and the staging dir it made.

use super::*;
use crate::scratch::Scratch;
use std::fs;

#[test]
fn a_staging_dir_is_one_name_in_bin_and_installs_only_its_own_files() {
    let dir = Scratch::new("write-setup-staging");
    let bin = dir.join("bin");
    let refused = Staging::fresh(&bin, "../escape").expect_err("a path is no name");
    assert_eq!(
        refused.to_string(),
        "`../escape` is not a staging dir name: it must be one plain file name"
    );
    assert!(!bin.exists(), "nothing was made");

    let staging = Staging::fresh(&bin, ".xtask-setup-tool-1").expect("a fresh staging dir");
    assert_eq!(staging.dir(), bin.join(".xtask-setup-tool-1"));
    Scratch::write(staging.dir().join("tool"), "binary").expect("a download");
    Scratch::write(dir.join("elsewhere"), "binary").expect("a file outside");
    let outside = staging
        .install(&dir.join("elsewhere"), "tool")
        .expect_err("only a file in the staging dir is installed");
    assert_eq!(
        outside.to_string(),
        format!(
            "{} is not directly in the staging dir {}, so it is not installed",
            dir.join("elsewhere").display(),
            staging.dir().display()
        )
    );
    let named = staging
        .install(&staging.dir().join("tool"), "../tool")
        .expect_err("a path is no binary name");
    assert_eq!(
        named.to_string(),
        "`../tool` is not a binary name: it must be one plain file name"
    );
    staging
        .install(&staging.dir().join("tool"), "tool")
        .expect("the install");
    assert_eq!(
        fs::read_to_string(bin.join("tool")).expect("installed"),
        "binary"
    );
    staging.remove().expect("the staging dir goes");
    assert!(!staging.dir().exists());
    staging
        .remove()
        .expect("a staging dir already gone is fine");
}

/// Makes `dir` read-only (true) or writable again (false).
#[cfg(unix)]
fn read_only(dir: &Path, on: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mode = if on { 0o555 } else { 0o755 };
    Scratch::set_permissions(dir, fs::Permissions::from_mode(mode)).expect("chmod the folder");
}

#[cfg(unix)]
#[test]
fn a_staging_dir_that_cannot_be_cleared_fails_fresh_with_the_os_error() {
    let dir = Scratch::new("write-setup-stuck-fresh");
    let bin = dir.join("bin");
    let staging = Staging::fresh(&bin, "stuck").expect("a fresh staging dir");
    Scratch::write(staging.dir().join("left"), "an earlier download").expect("a leftover");
    read_only(&bin, true);
    let refused = Staging::fresh(&bin, "stuck");
    read_only(&bin, false);
    let error = refused.expect_err("bin will not let the old staging dir go");
    assert_eq!(
        error.kind(),
        io::ErrorKind::PermissionDenied,
        "the OS error is returned, not a later create's"
    );
    assert!(
        staging.dir().exists(),
        "the dir it could not clear is still there"
    );
}

#[cfg(unix)]
#[test]
fn a_staging_dir_that_cannot_be_removed_fails_remove_with_the_os_error() {
    let dir = Scratch::new("write-setup-stuck-remove");
    let bin = dir.join("bin");
    let staging = Staging::fresh(&bin, "stuck").expect("a fresh staging dir");
    read_only(&bin, true);
    let removed = staging.remove();
    read_only(&bin, false);
    let error = removed.expect_err("bin will not let the staging dir go");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(staging.dir().exists());
}
