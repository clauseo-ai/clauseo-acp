//! Opening by descriptor. A path the watch is given is absolute and canonical; its folder is
//! opened with `O_NOFOLLOW_ANY`, which refuses a link anywhere in it, and the last name is
//! then read or changed relative to that folder's descriptor, never through a link. A folder
//! is entered from its parent's descriptor only after `fstatat` says it is a folder on the
//! same volume, or on a local volume that is not an automount.

use std::ffi::OsStr;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Component, Path, PathBuf};

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

/// The one volume a scan enters beyond local ones that are not automounts: the matter's own,
/// read from the matter folder's descriptor at launch, at and below the matter only (and in
/// the volume's `.clauseo` set-aside folder, where its items go). A matter on a network or
/// automounted volume (an after-only one, `Policy::matter_watch`) is scanned through it, even
/// under `/net` or `/Network`: names on that volume are looked up, since it is mounted
/// already. Every other such volume, a different one inside the matter included, stays
/// refused. For a matter on a local volume it adds nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Allow {
    root: PathBuf,
    /// Where the matter's volume is mounted.
    volume: PathBuf,
    dev: i32,
}

impl Allow {
    /// The allowance of matter `root`, read from its folder opened with no link followed.
    pub(crate) fn of(root: &Path) -> io::Result<Allow> {
        let fd = open_path(root)?;
        let dev = fstat(&fd)?.st_dev;
        let mounted = fstatfs(&fd)?.f_mntonname;
        let volume: Vec<u8> = mounted
            .iter()
            .take_while(|c| **c != 0)
            .map(|c| c.cast_unsigned())
            .collect();
        Ok(Allow {
            root: root.to_owned(),
            volume: PathBuf::from(std::ffi::OsString::from_vec(volume)),
            dev,
        })
    }

    /// Whether names in folder `dir` may be looked up though it is under `/net` or
    /// `/Network`: it is on the matter's volume, which is mounted.
    pub(super) fn covers(&self, dir: &Path) -> bool {
        dir.starts_with(&self.volume)
    }

    /// Whether folder `path` on volume `dev` may be entered by this allowance.
    fn admits(&self, path: &Path, dev: i32) -> bool {
        dev == self.dev
            && (path.starts_with(&self.root) || path.starts_with(self.volume.join(".clauseo")))
    }
}

/// Whether `allow` lets folder `path` on volume `dev` be entered.
fn admitted(allow: Option<&Allow>, path: &Path, dev: i32) -> bool {
    allow.is_some_and(|allow| allow.admits(path, dev))
}

#[cfg(test)]
std::thread_local! {
    /// The tests' volume flags, by device, read instead of the volume's own on this thread.
    static FAKED: std::cell::RefCell<Vec<(i32, u32)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Makes the volume checks read `flags` for device `dev`, on this thread.
#[cfg(test)]
pub(crate) fn fake_flags(dev: i32, flags: u32) {
    FAKED.with(|faked| faked.borrow_mut().push((dev, flags)));
}

/// The faked flags of device `dev`, if a test set them.
#[cfg(test)]
fn faked(dev: i32) -> Option<u32> {
    FAKED.with(|faked| {
        faked
            .borrow()
            .iter()
            .rev()
            .find(|(d, _)| *d == dev)
            .map(|(_, f)| *f)
    })
}

/// No flags are faked outside the tests.
#[cfg(not(test))]
const fn faked(_dev: i32) -> Option<u32> {
    None
}

/// Whether folder `fd` (on volume `dev`) is on a local volume that is not an automount.
fn plain_volume(fd: &OwnedFd, dev: i32) -> io::Result<bool> {
    if let Some(flags) = faked(dev) {
        return Ok(plain(flags));
    }
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
/// mount something. `allow` lifts both inside its matter.
pub(super) fn parent<'a>(
    path: &'a Path,
    plain: bool,
    allow: Option<&Allow>,
) -> io::Result<(OwnedFd, &'a OsStr)> {
    let canonical = path.is_absolute() && path.components().all(|c| c != Component::ParentDir);
    if !canonical {
        return Err(io::Error::other(format!(
            "{} is not an absolute path without `..`",
            path.display()
        )));
    }
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::other(format!(
            "{} has no folder",
            path.display()
        )));
    };
    let inside = allow.is_some_and(|allow| allow.covers(dir));
    if off_limits(path) && !inside {
        return Err(refused(path));
    }
    let fd = open_path(dir)?;
    if plain {
        let dev = fstat(&fd)?.st_dev;
        if !admitted(allow, dir, dev) && !plain_volume(&fd, dev)? {
            return Err(refused(dir));
        }
    }
    Ok((fd, name))
}

/// Opens folder `name` in `dir`, never through a link: `None` when it is missing, not a
/// folder, or swapped between the check and the open. A folder on another volume is opened
/// only when that volume is local and not an automount, which is read without entering it
/// (`attr.rs`; `fstatat` of a trigger doesn't fire it, see `os.rs`), or `allow` admits it.
pub(super) fn child(
    dir: &Folder,
    name: &OsStr,
    path: &Path,
    allow: Option<&Allow>,
) -> io::Result<Option<Folder>> {
    let Some(stat) = lstat_at(&dir.fd, name)? else {
        return Ok(None);
    };
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        return Ok(None);
    }
    if stat.st_dev != dir.dev && !admitted(allow, path, stat.st_dev) {
        let flags = match faked(stat.st_dev) {
            Some(flags) => flags,
            None => attr::mount_flags(&dir.fd, name)?,
        };
        if !plain(flags) {
            return Err(refused(path));
        }
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

/// Opens `allow`'s matter itself, when `path` is it: no folder above it is opened, which
/// may be on a volume the watch never enters. Refused when the matter is now on another
/// volume; `None` when it is missing.
fn open_root(path: &Path, allow: Option<&Allow>) -> Option<io::Result<Option<(Folder, Stat)>>> {
    let allow = allow.filter(|allow| allow.root == path)?;
    let fd = match open_path(path) {
        Ok(fd) => fd,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Some(Ok(None)),
        Err(error) => return Some(Err(error)),
    };
    Some(fstat(&fd).map_err(io::Error::from).and_then(|stat| {
        if stat.st_dev != allow.dev {
            return Err(refused(path));
        }
        Ok(Some((
            Folder {
                fd,
                dev: stat.st_dev,
            },
            stat,
        )))
    }))
}

/// Opens folder `path` for a scan: its folder with no link followed, then `path` itself as
/// [`child`] does; `allow`'s matter itself directly. Refused on a network volume or an
/// automount `allow` doesn't admit; `None` when it is not a folder.
pub(super) fn open_dir(path: &Path, allow: Option<&Allow>) -> io::Result<Option<(Folder, Stat)>> {
    if let Some(opened) = open_root(path, allow) {
        return opened;
    }
    let (fd, name) = parent(path, true, allow)?;
    let above = Folder {
        dev: fstat(&fd)?.st_dev,
        fd,
    };
    let Some(folder) = child(&above, name, path, allow)? else {
        return Ok(None);
    };
    let stat = fstat(&folder.fd)?;
    Ok(Some((folder, stat)))
}

/// The entry at `path`, never through a link, as [`parent`] with `plain` opens its folder;
/// `allow`'s matter itself from its own descriptor. `None` when it is missing.
pub(super) fn entry(path: &Path, allow: Option<&Allow>) -> io::Result<Option<Stat>> {
    if let Some(opened) = open_root(path, allow) {
        return opened.map(|found| found.map(|(_, stat)| stat));
    }
    let (dir, name) = parent(path, true, allow)?;
    lstat_at(&dir, name)
}
