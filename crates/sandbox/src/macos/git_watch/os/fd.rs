//! Opening by descriptor. A path the watch is given is absolute and canonical; its folder is
//! opened with `O_NOFOLLOW_ANY`, which refuses a link anywhere in it, and the last name is
//! then read or changed relative to that folder's descriptor, never through a link. A folder
//! is entered from its parent's descriptor only after `fstatat` says it is a folder on the
//! same volume, or on a local volume that is not an automount.

use std::ffi::OsStr;
use std::io;
use std::os::fd::OwnedFd;
use std::path::{Component, Path};

use rustix::fs::{AtFlags, CWD, FileType, Mode, OFlags, Stat, fstat, fstatfs, openat, statat};

use super::super::{Kind, Meta};
use super::attr;

/// `statfs`'s flag for a volume on this Mac (`<sys/mount.h>`).
const MNT_LOCAL: u32 = 0x0000_1000;

/// `statfs`'s flag for a volume the automounter mounted (`<sys/mount.h>`).
const MNT_AUTOMOUNTED: u32 = 0x0040_0000;

/// The flags every folder the watch opens is opened with: never through a link.
pub(super) const DIR: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// A folder held open, with its volume.
pub(crate) struct Folder {
    pub(super) fd: OwnedFd,
    pub(super) dev: i32,
}

/// What `kind` an entry of type `file_type` is.
pub(super) fn kind(file_type: FileType) -> Kind {
    match file_type {
        FileType::Symlink => Kind::Link,
        FileType::Directory => Kind::Dir,
        FileType::RegularFile => Kind::File,
        FileType::Fifo
        | FileType::Socket
        | FileType::CharacterDevice
        | FileType::BlockDevice
        | FileType::Unknown => Kind::Other,
    }
}

/// The volume of `stat`, as `std`'s `MetadataExt::dev` gives it.
pub(super) fn dev(stat: &Stat) -> u64 {
    i64::from(stat.st_dev).cast_unsigned()
}

/// The entry `stat` describes.
pub(super) fn meta(stat: &Stat) -> Meta {
    Meta {
        kind: kind(FileType::from_raw_mode(stat.st_mode)),
        id: (dev(stat), stat.st_ino),
    }
}

/// Whether volume flags `flags` are a local volume's that the automounter did not mount.
pub(super) fn plain(flags: u32) -> bool {
    flags & MNT_LOCAL != 0 && flags & MNT_AUTOMOUNTED == 0
}

/// Whether `path` is at or below `/net` or `/Network`, which the watch never touches: a
/// lookup there asks automountd, which may mount a server.
pub(crate) fn off_limits(path: &Path) -> bool {
    let mut parts = path.components();
    parts.next() == Some(Component::RootDir)
        && matches!(parts.next(), Some(Component::Normal(first))
            if first.eq_ignore_ascii_case("net") || first.eq_ignore_ascii_case("Network"))
}

/// The error for a folder on a volume the watch never enters.
pub(super) fn refused(what: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "{} is on a network volume or an automount, which the git watch never enters",
            what.display()
        ),
    )
}

/// The entry `name` in folder `dir`, never through a link; `None` when it is missing.
pub(super) fn lstat_at(dir: &OwnedFd, name: &OsStr) -> io::Result<Option<Stat>> {
    match statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => Ok(Some(stat)),
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Opens folder `path` with no link followed anywhere in it. `O_NOFOLLOW_ANY` goes without
/// `O_NOFOLLOW`: the kernel refuses the two together (`EINVAL`).
fn open_path(path: &Path) -> io::Result<OwnedFd> {
    let flags = DIR.difference(OFlags::NOFOLLOW) | OFlags::NOFOLLOW_ANY;
    Ok(openat(CWD, path, flags, Mode::empty())?)
}

/// Whether folder `fd` is on a local volume that is not an automount.
fn plain_volume(fd: &OwnedFd) -> io::Result<bool> {
    let stat = fstatfs(fd)?;
    let name: Vec<u8> = stat
        .f_fstypename
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| c.cast_unsigned())
        .collect();
    Ok(plain(stat.f_flags) && name != b"autofs")
}

/// The folder holding `path`, opened with no link followed, and `path`'s last name. Refused for
/// a path that isn't absolute, holds `..`, or is at or below `/net` or `/Network`; with
/// `plain`, also when the folder is on a network volume or an automount, where a lookup could
/// mount something.
pub(super) fn parent(path: &Path, plain: bool) -> io::Result<(OwnedFd, &OsStr)> {
    let canonical = path.is_absolute() && path.components().all(|c| c != Component::ParentDir);
    if !canonical {
        return Err(io::Error::other(format!(
            "{} is not an absolute path without `..`",
            path.display()
        )));
    }
    if off_limits(path) {
        return Err(refused(path));
    }
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::other(format!(
            "{} has no folder",
            path.display()
        )));
    };
    let fd = open_path(dir)?;
    if plain && !plain_volume(&fd)? {
        return Err(refused(dir));
    }
    Ok((fd, name))
}

/// Opens folder `name` in `dir`, never through a link: `None` when it is missing, not a
/// folder, or swapped between the check and the open. A folder on another volume is opened
/// only when that volume is local and not an automount, which is read without entering it
/// (`attr.rs`); `fstatat` of a trigger doesn't fire it (see `os.rs`).
pub(super) fn child(dir: &Folder, name: &OsStr, path: &Path) -> io::Result<Option<Folder>> {
    let Some(stat) = lstat_at(&dir.fd, name)? else {
        return Ok(None);
    };
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        return Ok(None);
    }
    if stat.st_dev != dir.dev && !plain(attr::mount_flags(&dir.fd, name)?) {
        return Err(refused(path));
    }
    let fd = match openat(&dir.fd, name, DIR, Mode::empty()) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT | rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) => {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let opened = fstat(&fd)?;
    if (opened.st_dev, opened.st_ino) != (stat.st_dev, stat.st_ino) {
        return Ok(None);
    }
    Ok(Some(Folder {
        fd,
        dev: opened.st_dev,
    }))
}

/// Opens folder `path` for a scan: its folder with no link followed, then `path` itself as
/// [`child`] does. Refused on a network volume or an automount; `None` when it is not a folder.
pub(super) fn open_dir(path: &Path) -> io::Result<Option<(Folder, Stat)>> {
    let (fd, name) = parent(path, true)?;
    let above = Folder {
        dev: fstat(&fd)?.st_dev,
        fd,
    };
    let Some(folder) = child(&above, name, path)? else {
        return Ok(None);
    };
    let stat = fstat(&folder.fd)?;
    Ok(Some((folder, stat)))
}
