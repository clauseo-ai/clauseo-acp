//! The std calls that change files, for the tests alone: making and removing each test's
//! folder, writing its `settings.json`, seeding matter folders and the lawyer's own edits, and
//! taking a folder's permissions away or moving it.
//! Workspace `clippy.toml` bans them everywhere else; the expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the host's tests make their folders and write their fixture files here alone"
)]

use std::path::Path;

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
}

/// `fs::remove_dir_all`, ignoring a folder that is already gone. Returns the error instead of
/// panicking, so a `Drop` can call it while a test unwinds.
pub(crate) fn remove_dir_all(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Unix: makes `link` a symbolic link to `target`.
#[cfg(unix)]
pub(crate) fn symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// Windows: opens `path` to read and write with a share mode that lets others read it but not
/// write it, as Word holds a document it has open.
#[cfg(windows)]
pub(crate) fn open_deny_write(path: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_SHARE_READ`.
    const SHARE_READ: u32 = 1;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(SHARE_READ)
        .open(path)
        .unwrap()
}

/// `fs::write`, creating the parent folder first.
pub(crate) fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

/// Makes `path` a file of `len` bytes with `File::set_len`: sparse where the file system
/// allows, so a file over the size cap costs no disk.
pub(crate) fn sized(path: &Path, len: u64) {
    std::fs::File::create(path).unwrap().set_len(len).unwrap();
}

/// Sets `path`'s modification time.
pub(crate) fn set_mtime(path: &Path, mtime: std::time::SystemTime) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(mtime))
        .unwrap();
}

/// `fs::rename`: the lawyer moving a folder.
pub(crate) fn rename(from: &Path, to: &Path) {
    std::fs::rename(from, to).unwrap();
}

/// `fs::remove_file`.
pub(crate) fn remove_file(path: &Path) {
    std::fs::remove_file(path).unwrap();
}

/// Sets `path`'s Unix permission bits.
#[cfg(unix)]
pub(crate) fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}
