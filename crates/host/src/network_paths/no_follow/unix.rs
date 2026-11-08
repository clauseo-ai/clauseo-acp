//! The no-follow open on Linux and macOS, through `rustix`'s safe calls: no `unsafe` here.
//!
//! The whole path is opened in one call that follows no link on it (any link fails with
//! `ELOOP`): on Linux `openat2` with `RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS`, on macOS
//! `open` with `O_NOFOLLOW_ANY` (macOS 11; the app needs 13, so its fallback is defensive only).
//! When that call is missing it goes name by name, remembered: on Linux a kernel before 5.6
//! (`ENOSYS`) or a seccomp sandbox that forbids `openat2` (`EPERM`, older Docker), on macOS
//! `EINVAL` or `ENOTSUP`. Name by name, each folder is `openat` from the last with
//! `O_NOFOLLOW | O_DIRECTORY`, the last name `O_NOFOLLOW`, and each one `fstat`ed, so a link
//! (which `O_PATH | O_NOFOLLOW` would hand back as itself) refuses too: as safe, only slower.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Component, Path, PathBuf};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::sync::atomic::{AtomicBool, Ordering};

use rustix::fs::{AtFlags, CWD, FileType, Mode, OFlags, Stat};
use rustix::io::Errno;

use super::{Access, Kind, link_on_the_way};
use crate::network_paths::resolve::Entry;

/// An open file descriptor, reached with no link followed.
#[derive(Debug)]
pub(super) struct Handle(OwnedFd);

/// Set once the one-call open answered that it is missing ([`MISSING`]): every later open goes
/// name by name.
#[cfg(any(target_os = "linux", target_os = "macos"))]
static NO_WHOLE: AtomicBool = AtomicBool::new(false);

/// The answers that say the one-call open is missing here, not that the path is refused: Linux's
/// `ENOSYS` (no `openat2`) and `EPERM` (a seccomp filter forbids it).
#[cfg(target_os = "linux")]
const MISSING: [Errno; 2] = [Errno::NOSYS, Errno::PERM];
/// macOS's: `EINVAL` or `ENOTSUP` for an `O_NOFOLLOW_ANY` the system does not know.
#[cfg(target_os = "macos")]
const MISSING: [Errno; 2] = [Errno::INVAL, Errno::NOTSUP];

/// The flags a folder on the way is opened with: a handle to walk from, nothing read.
#[cfg(target_os = "linux")]
const FOLDER: OFlags = OFlags::PATH.union(OFlags::DIRECTORY).union(OFlags::CLOEXEC);
/// The flags a folder on the way is opened with (macOS has no `O_PATH`).
#[cfg(not(target_os = "linux"))]
const FOLDER: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::CLOEXEC);

/// The flags the last name is opened with for `access` (`O_NOFOLLOW` added where it applies).
/// `O_NONBLOCK` and `O_NOCTTY`: a FIFO or a terminal never blocks or takes over the host.
fn flags(access: Access) -> OFlags {
    let read = OFlags::RDONLY
        .union(OFlags::CLOEXEC)
        .union(OFlags::NONBLOCK)
        .union(OFlags::NOCTTY);
    match access {
        #[cfg(target_os = "linux")]
        Access::Look => OFlags::PATH.union(OFlags::CLOEXEC),
        #[cfg(not(target_os = "linux"))]
        Access::Look => read,
        Access::Folder => FOLDER,
        Access::List => OFlags::RDONLY
            .union(OFlags::DIRECTORY)
            .union(OFlags::CLOEXEC),
        Access::Read | Access::Exclusive => read,
    }
}

/// Opens the absolute `path` for `access`, no name on it followed.
pub(super) fn open_path(path: &Path, access: Access) -> io::Result<Handle> {
    #[cfg(target_os = "linux")]
    {
        open_whole(path, access, &|path, flags| {
            rustix::fs::openat2(
                CWD,
                path,
                flags,
                Mode::empty(),
                rustix::fs::ResolveFlags::NO_SYMLINKS | rustix::fs::ResolveFlags::NO_MAGICLINKS,
            )
        })
    }
    #[cfg(target_os = "macos")]
    {
        open_whole(path, access, &|path, flags| {
            rustix::fs::openat(CWD, path, flags | OFlags::NOFOLLOW_ANY, Mode::empty())
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        by_name(path, access).map(Handle)
    }
}

/// The one-call open as [`open_path`] calls it (`openat2` on Linux, `open` with
/// `O_NOFOLLOW_ANY` on macOS): the path and the open flags, its no-follow flags its own.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) type Whole = dyn Fn(&Path, OFlags) -> rustix::io::Result<OwnedFd>;

/// The open: `whole` (a seam for the tests), or name by name once it answered that it is
/// missing ([`MISSING`], remembered). Never a plain open.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn open_whole(path: &Path, access: Access, whole: &Whole) -> io::Result<Handle> {
    plain_absolute(path)?;
    if !NO_WHOLE.load(Ordering::Relaxed) {
        match whole(path, flags(access)) {
            Ok(fd) => return checked(fd, access).map(Handle),
            Err(Errno::LOOP) => return Err(link_on_the_way()),
            Err(errno) if MISSING.contains(&errno) => {
                tracing::debug!(
                    kind = "no_follow_name_by_name",
                    errno = errno.raw_os_error(),
                    "the one-call no-follow open is missing; going name by name"
                );
                NO_WHOLE.store(true, Ordering::Relaxed);
            }
            Err(errno) => return Err(errno.into()),
        }
    }
    by_name(path, access).map(Handle)
}

/// Whether the one-call open was found missing (the tests' check that the fallback ran).
#[cfg(all(test, target_os = "linux"))]
pub(super) fn fell_back() -> bool {
    NO_WHOLE.load(Ordering::Relaxed)
}

/// Lets the next open try the one-call open again (tests).
#[cfg(all(test, target_os = "linux"))]
pub(super) fn reset_fallback() {
    NO_WHOLE.store(false, Ordering::Relaxed);
}

/// An error unless `path` is absolute with only plain names after its root (the walk's result
/// has no `.` or `..`).
fn plain_absolute(path: &Path) -> io::Result<()> {
    let plain = path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, Component::RootDir | Component::Normal(_)));
    if plain {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not an absolute path of plain names",
        ))
    }
}

/// The open name by name from `/`: each folder `O_NOFOLLOW | O_DIRECTORY`, the last name
/// `O_NOFOLLOW`, each checked not to be a link.
pub(super) fn by_name(path: &Path, access: Access) -> io::Result<OwnedFd> {
    plain_absolute(path)?;
    let names: Vec<&OsStr> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => None,
        })
        .collect();
    let Some((last, folders)) = names.split_last() else {
        let root = rustix::fs::openat(CWD, "/", flags(access), Mode::empty())?;
        return checked(root, access);
    };
    let mut at = rustix::fs::openat(CWD, "/", FOLDER, Mode::empty())?;
    for name in folders {
        at = open_name(&at, name, FOLDER)?;
    }
    let fd = open_name(&at, last, flags(access))?;
    checked(fd, access)
}

/// `name` in the folder `at` opened with `flags` and `O_NOFOLLOW`: a link refuses
/// ([`link_on_the_way`]), whether the open says so (`ELOOP`, or `ENOTDIR` for a link where a
/// folder was asked) or hands the link back as itself (`O_PATH`).
fn open_name(at: &OwnedFd, name: &OsStr, flags: OFlags) -> io::Result<OwnedFd> {
    match rustix::fs::openat(at, name, flags | OFlags::NOFOLLOW, Mode::empty()) {
        Ok(fd) => {
            if file_type(&rustix::fs::fstat(&fd)?) == FileType::Symlink {
                return Err(link_on_the_way());
            }
            Ok(fd)
        }
        Err(Errno::LOOP) => Err(link_on_the_way()),
        Err(errno @ Errno::NOTDIR) => {
            let is_link = rustix::fs::statat(at, name, AtFlags::SYMLINK_NOFOLLOW)
                .is_ok_and(|stat| file_type(&stat) == FileType::Symlink);
            Err(if is_link {
                link_on_the_way()
            } else {
                errno.into()
            })
        }
        Err(errno) => Err(errno.into()),
    }
}

/// `fd`, checked to be no link (an `O_PATH` open hands one back as itself), and a folder when
/// `access` lists one.
fn checked(fd: OwnedFd, access: Access) -> io::Result<OwnedFd> {
    let kind = file_type(&rustix::fs::fstat(&fd)?);
    if kind == FileType::Symlink {
        return Err(link_on_the_way());
    }
    if matches!(access, Access::Folder | Access::List) && kind != FileType::Directory {
        return Err(io::Error::from(Errno::NOTDIR));
    }
    Ok(fd)
}

/// The file type in `stat`.
fn file_type(stat: &Stat) -> FileType {
    FileType::from_raw_mode(stat.st_mode)
}

/// `stat`'s device and file number, as `std`'s `MetadataExt` gives them.
fn identity_of(stat: &Stat) -> (u64, u128) {
    #[cfg(target_os = "linux")]
    let dev = stat.st_dev;
    #[cfg(not(target_os = "linux"))]
    let dev = u64::from(stat.st_dev.cast_unsigned());
    (dev, u128::from(stat.st_ino))
}

impl Handle {
    pub(super) fn kind(&self) -> io::Result<Kind> {
        Ok(
            if file_type(&rustix::fs::fstat(&self.0)?) == FileType::Directory {
                Kind::Dir
            } else {
                Kind::File
            },
        )
    }

    pub(super) fn identity(&self) -> io::Result<(u64, u128)> {
        Ok(identity_of(&rustix::fs::fstat(&self.0)?))
    }

    pub(super) fn entry_at(&self, name: &OsStr) -> io::Result<Entry> {
        let stat = rustix::fs::statat(&self.0, name, AtFlags::SYMLINK_NOFOLLOW)?;
        Ok(match file_type(&stat) {
            FileType::Symlink => Entry::Link,
            FileType::Directory => Entry::Dir,
            FileType::RegularFile
            | FileType::Fifo
            | FileType::Socket
            | FileType::CharacterDevice
            | FileType::BlockDevice
            | FileType::Unknown => Entry::File,
        })
    }

    pub(super) fn read_link_at(&self, name: &OsStr) -> io::Result<PathBuf> {
        let target = rustix::fs::readlinkat(&self.0, name, Vec::new())?;
        Ok(PathBuf::from(OsStr::from_bytes(target.as_bytes())))
    }

    pub(super) fn identity_at(&self, name: &OsStr) -> io::Result<(u64, u128)> {
        Ok(identity_of(&rustix::fs::statat(
            &self.0,
            name,
            AtFlags::SYMLINK_NOFOLLOW,
        )?))
    }

    pub(super) fn open_at(&self, name: &OsStr, access: Access) -> io::Result<Self> {
        let fd = open_name(&self.0, name, flags(access))?;
        checked(fd, access).map(Self)
    }

    pub(super) fn entries(&self, max: usize) -> io::Result<Vec<(OsString, Option<Entry>)>> {
        let mut dir = rustix::fs::Dir::read_from(self.0.as_fd())?;
        let mut entries = Vec::new();
        while entries.len() < max {
            let Some(entry) = dir.read() else {
                break;
            };
            let entry = entry?;
            let name = OsStr::from_bytes(entry.file_name().to_bytes());
            if name == "." || name == ".." {
                continue;
            }
            let kind = match entry.file_type() {
                FileType::Symlink => Some(Entry::Link),
                FileType::Directory => Some(Entry::Dir),
                FileType::Unknown => self.entry_at(name).ok(),
                FileType::RegularFile
                | FileType::Fifo
                | FileType::Socket
                | FileType::CharacterDevice
                | FileType::BlockDevice => Some(Entry::File),
            };
            entries.push((name.to_os_string(), kind));
        }
        Ok(entries)
    }

    pub(super) fn into_file(self) -> File {
        File::from(self.0)
    }

    #[expect(
        clippy::unused_self,
        reason = "the same signature as macOS's, which reads the folder's volume"
    )]
    #[cfg(not(target_os = "macos"))]
    pub(super) fn local_apfs_or_hfs(&self) -> Option<bool> {
        None
    }

    /// `MNT_LOCAL` in the folder's `fstatfs` flags, and `apfs` or `hfs` as its type.
    #[cfg(target_os = "macos")]
    pub(super) fn local_apfs_or_hfs(&self) -> Option<bool> {
        let stat = rustix::fs::fstatfs(&self.0).ok()?;
        let name: Vec<u8> = stat
            .f_fstypename
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| u8::from_ne_bytes(c.to_ne_bytes()))
            .collect();
        let local = stat.f_flags & u32::try_from(libc::MNT_LOCAL).ok()? != 0;
        Some(local && matches!(name.as_slice(), b"apfs" | b"hfs"))
    }
}
