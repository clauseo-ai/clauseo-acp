//! Unix: files made inside a folder opened once, so a link swapped in for the folder after it
//! was checked is never written through: a file replaced (`browser_guard/chrome/profile.rs`),
//! a download moved or copied in and a note made (`browser_guard/downloads.rs`,
//! `write/copy.rs`), a folder made (`browser_guard.rs`).

use std::ffi::{CString, OsStr};
use std::io::{self, Write};
use std::os::fd::OwnedFd;
use std::path::Path;

use nix::dir::Dir;
use nix::errno::Errno;
use nix::fcntl::{AT_FDCWD, AtFlags, OFlag, open, openat, renameat};
use nix::sys::stat::{Mode, fstatat, mkdirat};
use nix::unistd::{UnlinkatFlags, linkat, unlinkat};

/// Opens `folder` itself with `O_DIRECTORY|O_NOFOLLOW`: a link at its name fails with
/// `ELOOP`, anything but a folder with `ENOTDIR`. Only the last part of the path is held
/// to that; the folders above it are resolved as they are.
pub(super) fn open_folder(folder: &Path) -> io::Result<OwnedFd> {
    open(
        folder,
        OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_RDONLY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(io::Error::from)
}

/// Whether `name` exists in the folder `dir` holds, as anything, a link included
/// (`fstatat` with `AT_SYMLINK_NOFOLLOW`).
pub(super) fn exists_in(dir: &OwnedFd, name: &OsStr) -> bool {
    fstatat(dir, name, AtFlags::AT_SYMLINK_NOFOLLOW).is_ok()
}

/// Moves the file `from` to `name` in the folder `dir` holds, never replacing anything there:
/// `linkat(AT_FDCWD, from, dir, name, 0)` (fails with `EEXIST` for a taken name; without
/// `AT_SYMLINK_FOLLOW` it never follows a link at `from`), then `from` is unlinked. When that
/// fails, the new name goes again (`unlinkat(dir, name)`, through the handle: never a path that
/// could have become a link meanwhile), so the file keeps one name and can be tried again.
pub(super) fn move_new_into(from: &Path, dir: &OwnedFd, name: &OsStr) -> io::Result<()> {
    linkat(AT_FDCWD, from, dir, name, AtFlags::empty()).map_err(io::Error::from)?;
    let removed = std::fs::remove_file(from);
    if removed.is_err() {
        // `name` is the link just made, which nothing else had.
        if unlinkat(dir, name, UnlinkatFlags::NoRemoveDir).is_err() {}
    }
    removed
}

/// Makes the file `name` in the folder `dir` holds, new (`O_CREAT|O_EXCL|O_NOFOLLOW`), mode
/// 0666 less the umask, as `File::create_new` would; open for reading too (`O_RDWR`).
pub(super) fn create_new_in(dir: &OwnedFd, name: &OsStr) -> io::Result<std::fs::File> {
    let file: OwnedFd = openat(
        dir,
        name,
        OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_RDWR | OFlag::O_CLOEXEC,
        Mode::from_bits_truncate(0o666),
    )
    .map_err(io::Error::from)?;
    Ok(std::fs::File::from(file))
}

/// Whether a failed hard link means this file system has none (FAT and exFAT: `EPERM`; others
/// `ENOTSUP`, `EOPNOTSUPP` or `ENOSYS`; `EMLINK` for a file at its link limit): the move then
/// copies.
pub(super) fn no_hard_links(error: &io::Error) -> bool {
    not_supported(error)
        || matches!(
            error.raw_os_error().map(Errno::from_raw),
            Some(Errno::EPERM | Errno::ENOSYS | Errno::EMLINK)
        )
}

/// Whether `error` is `ENOTSUP` or `EOPNOTSUPP`: one number on Linux (95), two on macOS
/// (45 and 102), where exFAT, FAT and SMB answer `ENOTSUP`.
pub(crate) fn not_supported(error: &io::Error) -> bool {
    error
        .raw_os_error()
        .is_some_and(|code| code == nix::libc::ENOTSUP || code == nix::libc::EOPNOTSUPP)
}

/// Whether the file `name` in the folder `dir` holds, not followed if it is a link, is the open
/// `file` (same device and inode): a published copy that is still the one this process made.
pub(super) fn is_same_file_in(
    dir: &OwnedFd,
    name: &OsStr,
    file: &std::fs::File,
) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let found = fstatat(dir, name, AtFlags::AT_SYMLINK_NOFOLLOW).map_err(io::Error::from)?;
    let held = file.metadata()?;
    // `st_dev` is a `u64` on Linux and an `i32` on macOS: compared as `i128`, losslessly.
    Ok(i128::from(found.st_dev) == i128::from(held.dev())
        && i128::from(found.st_ino) == i128::from(held.ino()))
}

/// Removes `name` from the folder `dir` holds when it is a plain file (`fstatat` with
/// `AT_SYMLINK_NOFOLLOW`), through the handle: a link is never followed (one swapped in after
/// the look is itself unlinked, never its target), a folder never removed.
pub(super) fn remove_plain_in(dir: &OwnedFd, name: &OsStr) -> io::Result<()> {
    let found = fstatat(dir, name, AtFlags::AT_SYMLINK_NOFOLLOW).map_err(io::Error::from)?;
    if nix::sys::stat::SFlag::from_bits_truncate(found.st_mode) & nix::sys::stat::SFlag::S_IFMT
        != nix::sys::stat::SFlag::S_IFREG
    {
        return Err(io::Error::other("not a plain file"));
    }
    remove_in(dir, name)
}

/// The names in the folder `dir` holds, read through a second handle opened relative to it
/// (`openat(dir, ".")`, then `fdopendir`), never its path. Empty when the listing cannot open.
pub(super) fn names_in(dir: &OwnedFd) -> Vec<std::ffi::OsString> {
    use std::os::unix::ffi::OsStrExt;
    let Ok(again) = openat(
        dir,
        ".",
        OFlag::O_DIRECTORY | OFlag::O_RDONLY | OFlag::O_CLOEXEC,
        Mode::empty(),
    ) else {
        return Vec::new();
    };
    let Ok(mut listing) = Dir::from_fd(again) else {
        return Vec::new();
    };
    listing
        .iter()
        .flatten()
        .map(|entry| OsStr::from_bytes(entry.file_name().to_bytes()).to_owned())
        .filter(|name| name != "." && name != "..")
        .collect()
}

/// Opens the file `from` to copy it: `O_NOFOLLOW` (a link fails), `O_NONBLOCK` (a FIFO does not
/// block the open), then anything but a plain file is refused.
pub(super) fn open_source(from: &Path) -> io::Result<std::fs::File> {
    let file: OwnedFd = open(
        from,
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    let file = std::fs::File::from(file);
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a plain file"));
    }
    Ok(file)
}

/// `unlinkat(dir, name)`: a name this process made in the folder, never a folder.
pub(super) fn remove_in(dir: &OwnedFd, name: &OsStr) -> io::Result<()> {
    unlinkat(dir, name, UnlinkatFlags::NoRemoveDir).map_err(io::Error::from)
}

/// Renames `from` to `to`, both in the folder `dir` holds, failing with `EEXIST` rather than
/// replacing anything at `to`: `renameat2(RENAME_NOREPLACE)` on Linux, `renameatx_np(RENAME_EXCL)`
/// on macOS. Where the call or the file system has no such rename (`EINVAL`, `ENOTSUP`,
/// `ENOSYS`; and on every other Unix), a hard link (`linkat`, which never replaces) then the
/// unlink of `from`. `Unsupported` when the file system has neither.
pub(super) fn rename_new_in(dir: &OwnedFd, from: &OsStr, to: &OsStr) -> io::Result<()> {
    match rename_exclusive(dir, from, to) {
        Err(error) if rename_unsupported(&error) => {}
        done => return done,
    }
    match linkat(dir, from, dir, to, AtFlags::empty()) {
        Ok(()) => {
            // `to` holds the file now; a `from` that stays is a hidden leftover.
            drop(remove_in(dir, from));
            Ok(())
        }
        Err(error) => {
            let error = io::Error::from(error);
            if no_hard_links(&error) {
                Err(io::Error::new(io::ErrorKind::Unsupported, error))
            } else {
                Err(error)
            }
        }
    }
}

/// Whether a no-replace rename failed only because it is not available here.
fn rename_unsupported(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error().map(Errno::from_raw),
        Some(Errno::EINVAL | Errno::ENOSYS)
    ) || not_supported(error)
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn rename_exclusive(dir: &OwnedFd, from: &OsStr, to: &OsStr) -> io::Result<()> {
    nix::fcntl::renameat2(
        dir,
        from,
        dir,
        to,
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(io::Error::from)
}

#[cfg(target_os = "macos")]
fn rename_exclusive(dir: &OwnedFd, from: &OsStr, to: &OsStr) -> io::Result<()> {
    super::mac::rename_excl(dir, from, to)
}

#[cfg(not(any(all(target_os = "linux", target_env = "gnu"), target_os = "macos")))]
fn rename_exclusive(_dir: &OwnedFd, _from: &OsStr, _to: &OsStr) -> io::Result<()> {
    Err(Errno::ENOSYS.into())
}

/// Makes the folder `name` in `parent` if it is missing, then insists it is a real folder: the
/// parent is opened once (`O_DIRECTORY`, following a link at its own name: the guard is given
/// it, and the agent cannot rename it), the folder made with `mkdirat` (which never follows a
/// link at `name`: `EEXIST`), then opened with `openat(O_DIRECTORY|O_NOFOLLOW)` through the
/// same handle, so a link or a file at `name` is refused.
pub(super) fn make_folder_in(parent: &Path, name: &OsStr) -> io::Result<()> {
    let dir = open(
        parent,
        OFlag::O_DIRECTORY | OFlag::O_RDONLY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    match mkdirat(&dir, name, Mode::from_bits_truncate(0o777)) {
        Ok(()) | Err(Errno::EEXIST) => {}
        Err(error) => return Err(error.into()),
    }
    match openat(
        &dir,
        name,
        OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_RDONLY | OFlag::O_CLOEXEC,
        Mode::empty(),
    ) {
        Ok(_folder) => Ok(()),
        Err(Errno::ELOOP | Errno::ENOTDIR) => {
            Err(io::Error::other("it exists and is not a folder"))
        }
        Err(error) => Err(error.into()),
    }
}

/// Writes `contents` to `name` in `folder`: opens `folder` itself with `O_DIRECTORY|O_NOFOLLOW`
/// (a link there fails the open), removes the temporary files an earlier write left
/// ([`super::temporary_prefix`]: a write killed midway), makes this write's own temporary file
/// ([`super::temporary_name`], unique to this process and call) with
/// `O_CREAT|O_EXCL|O_NOFOLLOW`, then `renameat`s it over `name` within the same folder, which
/// replaces a link at `name` instead of following it. Every step names the file relative to the
/// one open folder, so nothing that happens to `folder`'s path after the open moves the write.
/// The file is mode 0600. A rename on Unix is not refused by a process that holds `name` open.
pub(super) fn replace_in_folder(folder: &Path, name: &OsStr, contents: &[u8]) -> io::Result<()> {
    let dir = open_folder(folder)?;
    remove_left(&dir, name);
    let temporary = super::temporary_name(name);
    let file: OwnedFd = openat(
        &dir,
        temporary.as_os_str(),
        OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_WRONLY | OFlag::O_CLOEXEC,
        Mode::S_IRUSR | Mode::S_IWUSR,
    )
    .map_err(io::Error::from)?;
    let mut file = std::fs::File::from(file);
    let written = file.write_all(contents);
    drop(file);
    let replaced = written
        .and_then(|()| renameat(&dir, temporary.as_os_str(), &dir, name).map_err(io::Error::from));
    if replaced.is_err() {
        // Only this write's own temporary file goes; the old file stays as it was, and a
        // temporary file that cannot go now is removed by the next write.
        if unlinkat(&dir, temporary.as_os_str(), UnlinkatFlags::NoRemoveDir).is_err() {}
    }
    replaced
}

/// Unlinks every entry of the folder `dir` holds whose name starts with `name`'s temporary
/// prefix: files (or links) an earlier write left when it was killed. Both the listing and the
/// unlinks go through the open folder, never its path: the listing reads a second handle opened
/// relative to `dir` (`openat(dir, ".")`, then `fdopendir`). One that fails (a folder of that
/// name, an entry already gone, a listing that cannot open) is left, harmless: this write's own
/// name is new.
fn remove_left(dir: &OwnedFd, name: &OsStr) {
    let prefix = super::temporary_prefix(name);
    let Ok(again) = openat(
        dir,
        ".",
        OFlag::O_DIRECTORY | OFlag::O_RDONLY | OFlag::O_CLOEXEC,
        Mode::empty(),
    ) else {
        return;
    };
    let Ok(mut listing) = Dir::from_fd(again) else {
        return;
    };
    // Every name first, then the unlinks: the listing does not change while it is read.
    let left: Vec<CString> = listing
        .iter()
        .flatten()
        .map(|entry| entry.file_name().to_owned())
        .filter(|found| found.to_bytes().starts_with(prefix.as_encoded_bytes()))
        .collect();
    for found in left {
        // Gone already, or not a file: left, harmless.
        if unlinkat(dir, found.as_c_str(), UnlinkatFlags::NoRemoveDir).is_err() {}
    }
}
