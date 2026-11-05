//! The tests' links, locks and modes. On Windows a folder link is a directory junction, made
//! with `mklink /J` (no admin rights needed), the kind NSIS's `RMDir /r` followed; elsewhere a
//! symbolic link stands in.
#![expect(
    clippy::disallowed_methods,
    reason = "test set-up makes links, locks and modes in its scratch folder; no write module here"
)]

use std::io;
use std::path::Path;

/// Makes `link` (which must not exist) a link to the folder `target`: a junction on Windows,
/// which needs no privilege, so failing to make one fails the test. Both paths go to `cmd` with
/// backslashes only (it reads a `/` as a switch) and in quotes, as one raw argument.
#[cfg(windows)]
pub(super) fn folder_link(target: &Path, link: &Path) {
    use std::os::windows::process::CommandExt as _;
    let native = |path: &Path| path.components().collect::<std::path::PathBuf>();
    let (link, target) = (native(link), native(target));
    let line = format!(
        "/C mklink /J \"{}\" \"{}\"",
        link.display(),
        target.display()
    );
    let made = std::process::Command::new("cmd")
        .raw_arg(&line)
        .status()
        .unwrap();
    assert!(made.success(), "cmd {line}");
    // A reparse point (FILE_ATTRIBUTE_REPARSE_POINT), not a plain folder.
    let attributes = std::os::windows::fs::MetadataExt::file_attributes(
        &std::fs::symlink_metadata(&link).unwrap(),
    );
    assert!(attributes & 0x400 != 0, "cmd {line} made no junction");
}

/// Makes `link` (which must not exist) a symbolic link to the folder `target`.
#[cfg(not(windows))]
pub(super) fn folder_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// Whether `made` made its symbolic link. The one failure that skips: Windows without the
/// privilege to make one (`ERROR_PRIVILEGE_NOT_HELD`), said on stderr. Any other failure fails
/// the test.
pub(super) fn made_or_unprivileged(made: io::Result<()>) -> bool {
    /// `ERROR_PRIVILEGE_NOT_HELD`.
    const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;
    match made {
        Ok(()) => true,
        Err(error) if cfg!(windows) && error.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD) => {
            eprintln!("skipped: no privilege to make a symbolic link here ({error})");
            false
        }
        Err(error) => panic!("the symbolic link was not made: {error}"),
    }
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

/// Makes the empty folder `folder` a reparse point with `tag`, a tag of no Microsoft kind and
/// no name surrogate (`FSCTL_SET_REPARSE_POINT` with a `REPARSE_GUID_DATA_BUFFER`), so no
/// filter acts on it. The caller skips when the file system refuses.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "FFI: DeviceIoControl sets the test's reparse point"
)]
pub(super) fn foreign_reparse_point(folder: &Path, tag: u32) -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::IO::DeviceIoControl;
    /// `FSCTL_SET_REPARSE_POINT`.
    const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
    /// `FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES`.
    const ACCESS: u32 = 0x2 | 0x80 | 0x100;
    /// `FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT`.
    const FLAGS: u32 = 0x0200_0000 | 0x0020_0000;
    let file = std::fs::OpenOptions::new()
        .access_mode(ACCESS)
        .custom_flags(FLAGS)
        .open(folder)?;
    let data = *b"test";
    // ReparseTag, ReparseDataLength, Reserved, ReparseGuid, then the data.
    let mut buffer = Vec::new();
    buffer.extend_from_slice(&tag.to_le_bytes());
    buffer.extend_from_slice(&u16::try_from(data.len()).unwrap().to_le_bytes());
    buffer.extend_from_slice(&0u16.to_le_bytes());
    buffer.extend_from_slice(b"clauseo-test-tag");
    buffer.extend_from_slice(&data);
    let len = u32::try_from(buffer.len()).unwrap();
    let mut returned = 0u32;
    // SAFETY: the handle is open for the call (`file` lives past it); `buffer` is live and
    // `len` bytes long; no output buffer; `returned` is a live out-pointer, as a call without
    // an OVERLAPPED needs.
    unsafe {
        DeviceIoControl(
            HANDLE(file.as_raw_handle()),
            FSCTL_SET_REPARSE_POINT,
            Some(buffer.as_ptr().cast()),
            len,
            None,
            0,
            Some(&raw mut returned),
            None,
        )
    }
    .map_err(io::Error::from)
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

/// Renames `from` to `to`.
#[cfg(windows)]
pub(super) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
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
