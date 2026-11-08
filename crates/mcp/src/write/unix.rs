//! Unix: a file replaced inside a folder opened once, so a link swapped in for the folder after
//! it was checked is never written through (`browser_guard/chrome/profile.rs`).

use std::ffi::{CString, OsStr};
use std::io::{self, Write};
use std::os::fd::OwnedFd;
use std::path::Path;

use nix::dir::Dir;
use nix::fcntl::{OFlag, open, openat, renameat};
use nix::sys::stat::Mode;
use nix::unistd::{UnlinkatFlags, unlinkat};

/// Writes `contents` to `name` in `folder`: opens `folder` itself with `O_DIRECTORY|O_NOFOLLOW`
/// (a link there fails the open), removes the temporary files an earlier write left
/// ([`super::temporary_prefix`]: a write killed midway), makes this write's own temporary file
/// ([`super::temporary_name`], unique to this process and call) with
/// `O_CREAT|O_EXCL|O_NOFOLLOW`, then `renameat`s it over `name` within the same folder, which
/// replaces a link at `name` instead of following it. Every step names the file relative to the
/// one open folder, so nothing that happens to `folder`'s path after the open moves the write.
/// The file is mode 0600. A rename on Unix is not refused by a process that holds `name` open.
pub(super) fn replace_in_folder(folder: &Path, name: &OsStr, contents: &[u8]) -> io::Result<()> {
    let dir = open(
        folder,
        OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_RDONLY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
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
