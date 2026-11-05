//! The tests' links, locks and modes. On Windows a folder link is a directory junction, made
//! with `mklink /J` (no admin rights needed), the kind NSIS's `RMDir /r` followed; elsewhere a
//! symbolic link stands in.
#![expect(
    clippy::disallowed_methods,
    reason = "test set-up makes links, locks and modes in its scratch folder; no write module here"
)]

use std::io;
use std::path::Path;

/// Makes `link` (which must not exist) a link to the folder `target`: a junction on Windows.
#[cfg(windows)]
pub(super) fn folder_link(target: &Path, link: &Path) {
    let made = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .status()
        .unwrap();
    assert!(made.success(), "mklink /J {}", link.display());
}

/// Makes `link` (which must not exist) a symbolic link to the folder `target`.
#[cfg(not(windows))]
pub(super) fn folder_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// Makes `link` a directory symbolic link to `target`; Windows may refuse without the privilege.
#[cfg(windows)]
pub(super) fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

/// Makes `link` a file symbolic link to `target`; Windows may refuse without the privilege.
#[cfg(windows)]
pub(super) fn symlink_file(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

/// Makes `link` a symbolic link to the file `target`.
#[cfg(not(windows))]
pub(super) fn symlink_file(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Opens `file` with no sharing, so nothing can delete it while the handle lives.
#[cfg(windows)]
pub(super) fn lock(file: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(file)
        .unwrap()
}

/// Sets the read-only attribute of `path`.
#[cfg(windows)]
pub(super) fn set_read_only(path: &Path) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(path, permissions).unwrap();
}

/// Sets `folder`'s mode: 0o555 lets nothing in it be removed (for a user who is not root).
#[cfg(not(windows))]
pub(super) fn set_mode(folder: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Whether a file can still be made in `folder` (true for root, whatever the mode).
#[cfg(not(windows))]
pub(super) fn can_write_in(folder: &Path) -> bool {
    let probe = folder.join("probe");
    let made = std::fs::File::create(&probe).is_ok();
    if made {
        std::fs::remove_file(&probe).unwrap();
    }
    made
}

/// Removes the empty folder (or folder link) `path`.
pub(super) fn remove_dir(path: &Path) {
    std::fs::remove_dir(path).unwrap();
}

/// Removes the file `path`.
pub(super) fn remove_file(path: &Path) {
    std::fs::remove_file(path).unwrap();
}
