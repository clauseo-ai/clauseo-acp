//! The Unix half of the pack read: a walk by file descriptor through `rustix`'s safe calls.
//! Each folder is opened relative to its parent's descriptor, never through a link, and each
//! entry is examined and opened relative to the same descriptor, so no path is resolved twice.

use std::fs::File;
use std::os::fd::OwnedFd;
use std::path::Path;

use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, Stat};
use rustix::io::Errno;

use super::read::{Kind, check_root};
use super::{Refusal, refuse};

/// Flags for a folder: read, a folder only, never a link, closed on exec.
const FOLDER: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// Flags for a file: read, never a link, never blocking on a pipe swapped in, never a
/// controlling terminal, closed on exec.
const FILE: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::NOCTTY)
    .union(OFlags::CLOEXEC);

/// An open folder of the pack.
pub(super) struct Folder {
    fd: OwnedFd,
}

impl Folder {
    /// The pack's own folder: a folder and no link, by its path and by its descriptor.
    pub(super) fn open_root(folder: &Path) -> Result<Self, Refusal> {
        check_root(folder)?;
        let fd = rustix::fs::open(folder, FOLDER, Mode::empty()).map_err(open_error)?;
        Ok(Self { fd })
    }

    /// The names in the folder, `.` and `..` left out. A name that is not UTF-8 is refused.
    pub(super) fn names(&self) -> Result<Vec<String>, Refusal> {
        let mut names = Vec::new();
        for entry in Dir::read_from(&self.fd).map_err(refuse(Refusal::Io))? {
            let entry = entry.map_err(refuse(Refusal::Io))?;
            let name = entry.file_name().to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            let name = std::str::from_utf8(name).map_err(refuse(Refusal::BadPath))?;
            names.push(name.to_owned());
        }
        Ok(names)
    }

    /// What `name` is, from `lstat` relative to the folder.
    pub(super) fn kind(&self, name: &str) -> Result<Kind, Refusal> {
        let stat = self.stat(name)?;
        Ok(match FileType::from_raw_mode(stat.st_mode) {
            FileType::Symlink => Kind::Link,
            FileType::Directory => Kind::Folder,
            FileType::RegularFile => Kind::File {
                len: u64::try_from(stat.st_size).map_err(refuse(Refusal::Io))?,
                hard_linked: stat.st_nlink > 1,
            },
            _ => Kind::Other,
        })
    }

    /// The folder `name` inside, opened relative to this one, never through a link; it must be
    /// the folder `lstat` saw.
    pub(super) fn open_folder(&self, name: &str, _rel: &str) -> Result<Self, Refusal> {
        let seen = self.stat(name)?;
        let fd = rustix::fs::openat(&self.fd, name, FOLDER, Mode::empty()).map_err(open_error)?;
        let held = rustix::fs::fstat(&fd).map_err(refuse(Refusal::Io))?;
        if (held.st_dev, held.st_ino) != (seen.st_dev, seen.st_ino) {
            return Err(Refusal::Link);
        }
        Ok(Self { fd })
    }

    /// The file `name` inside, opened relative to this folder, never through a link; it must be
    /// the file `lstat` saw, still a plain file with one link. Returns it and whether any exec
    /// bit is set.
    pub(super) fn open_file(&self, name: &str, _rel: &str) -> Result<(File, bool), Refusal> {
        let seen = self.stat(name)?;
        let fd = rustix::fs::openat(&self.fd, name, FILE, Mode::empty()).map_err(open_error)?;
        let held = rustix::fs::fstat(&fd).map_err(refuse(Refusal::Io))?;
        if FileType::from_raw_mode(held.st_mode) != FileType::RegularFile {
            return Err(Refusal::NotAFile);
        }
        if (held.st_dev, held.st_ino) != (seen.st_dev, seen.st_ino) {
            return Err(Refusal::Link);
        }
        if held.st_nlink > 1 {
            return Err(Refusal::HardLink);
        }
        let exec = held.st_mode & 0o111 != 0;
        Ok((File::from(fd), exec))
    }

    /// `lstat` of `name`, relative to the folder.
    fn stat(&self, name: &str) -> Result<Stat, Refusal> {
        rustix::fs::statat(&self.fd, name, AtFlags::SYMLINK_NOFOLLOW).map_err(refuse(Refusal::Io))
    }
}

/// An open refused because the name is a link (`ELOOP`, or `ENOTDIR` for a link where a folder
/// was asked for) is [`Refusal::Link`]; any other failure is [`Refusal::Io`].
fn open_error(errno: Errno) -> Refusal {
    if errno == Errno::LOOP || errno == Errno::NOTDIR {
        Refusal::Link
    } else {
        Refusal::Io
    }
}
