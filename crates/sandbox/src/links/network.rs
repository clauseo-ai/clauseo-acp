//! Network paths the walk never touches. A `gitdir:` line, a `commondir` file or a link can
//! name a server's share (`\\host\share`, a mapped drive, an SMB or NFS mount); resolving it at
//! launch would connect to that server, which can send the user's NTLM hash or hang the launch.
//! So every such path is classified by its text and the mount table, name by name and link by
//! link ([`link_to_network`]), and one that leaves the local disks is not touched.
//!
//! On Linux no mount table is read: only a path's text marks it as a network path. Linux is not
//! a supported platform for this check (decision 0049).

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::Error;
use crate::policy::{same, under};

pub(crate) mod drives;

pub(crate) use drives::Drive;

/// The mount table, read without touching any server: macOS's mount list and Windows's drive
/// letters. Elsewhere it is empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Mounts {
    /// Mount points, each with whether its file system is a network one (not known local).
    points: Vec<(PathBuf, bool)>,
    /// The drive letters that have a device, upper case, and what each is.
    drives: Vec<(u8, Drive)>,
    /// Whether the mount list couldn't be read: the walk then fails, and the policy keeps every
    /// link target as written.
    pub(crate) unread: bool,
}

/// The macOS file system types known to be local: disks, Apple's `lifs` (its user-space driver
/// for FAT and exFAT media), read-only NTFS, and optical media (`udf`, `cd9660`). Any other
/// (`smbfs`, `nfs`, `autofs`, macFUSE's sshfs, `nullfs`, bindfs) counts as network: it may
/// reach a server, or another mount.
#[cfg(any(target_os = "macos", test))]
const LOCAL_FS: [&str; 9] = [
    "apfs", "hfs", "msdos", "exfat", "devfs", "lifs", "ntfs", "udf", "cd9660",
];

/// The most `subst` mappings one path goes through before it counts as unknown.
const SUBST_MAX: usize = 8;

/// The most links one chain follows; past it the path counts as unknown. At least the kernel's
/// own limit (macOS 32, Linux 40, Windows 63 reparse points), so the check never gives up on a
/// chain the open would still follow.
#[cfg(windows)]
const HOPS_MAX: usize = 63;
#[cfg(not(windows))]
const HOPS_MAX: usize = 40;

impl Mounts {
    /// A table of these mount points (with whether each is a network one) and remote drives.
    #[cfg(test)]
    pub(crate) fn new(points: Vec<(PathBuf, bool)>, remote_drives: &[u8]) -> Mounts {
        let drives = remote_drives
            .iter()
            .map(|letter| (letter.to_ascii_uppercase(), Drive::Network))
            .collect();
        Mounts {
            points,
            drives,
            unread: false,
        }
    }

    /// A table of these mount points, each with its file system type (`f_fstypename`).
    #[cfg(test)]
    pub(crate) fn of_kinds(points: &[(&str, &str)]) -> Mounts {
        let points = points
            .iter()
            .map(|(point, kind)| (PathBuf::from(point), Mounts::is_network_fs(kind)))
            .collect();
        Mounts::new(points, &[])
    }

    /// This table with drive `letter` set to `drive`.
    #[cfg(test)]
    pub(crate) fn with_drive(mut self, letter: u8, drive: Drive) -> Mounts {
        self.drives.push((letter.to_ascii_uppercase(), drive));
        self
    }

    /// Whether the file system type `kind` (a mount's `f_fstypename`) is not known local.
    #[cfg(any(target_os = "macos", test))]
    fn is_network_fs(kind: &str) -> bool {
        !LOCAL_FS.contains(&kind)
    }

    /// This machine's table: on macOS the mount list (`getfsstat` with `MNT_NOWAIT`, which
    /// answers from the kernel's table and asks no server), on Windows each drive letter's DOS
    /// device ([`drives`]). [`Mounts::unread`] if the mount list can't be read.
    pub(crate) fn current() -> Mounts {
        os::current()
    }

    /// The network mount points strictly inside `matter`.
    fn network_inside(&self, matter: &Path) -> Vec<PathBuf> {
        self.points
            .iter()
            .filter(|(point, network)| *network && under(point, matter) && !same(point, matter))
            .map(|(point, _)| point.clone())
            .collect()
    }

    /// What drive `letter` is; a letter with no device is local (nothing there to touch).
    fn drive(&self, letter: u8) -> &Drive {
        let letter = letter.to_ascii_uppercase();
        self.drives
            .iter()
            .find(|(have, _)| *have == letter)
            .map_or(&Drive::Local, |(_, drive)| drive)
    }
}

/// Refuses a protected launch for a matter on a network drive or share (a mapped drive, a
/// `subst` onto one, `\\server\share`, `\\?\UNC\…`, a non-local macOS mount), or holding a
/// network mount, by [`classify`] and `mounts` alone, so before any file call on it. The agent
/// writes anywhere in the matter, and a server holds what the jail can't guard; sharing
/// matters over a network comes later. A matter on a path the launch can't check by its text
/// (a volume or device name, a `..` in a `\\?\` path) refuses too. On Windows a link in the
/// matter to a share is a link finding, never followed.
pub(crate) fn check_matter_local(matter: &Path, mounts: &Mounts) -> Result<(), Error> {
    if is_network_path(matter, mounts) || !mounts.network_inside(matter).is_empty() {
        tracing::info!("the sandbox refused a matter on a network drive; nothing was planned");
        return Err(Error::MatterOnNetwork);
    }
    Ok(())
}

/// Where `path` leaves the local disks by [`link_to_network`] with the file system's calls:
/// the check run on a settings path before anything else touches it.
pub(crate) fn off_local(path: &Path, mounts: &Mounts) -> Option<(PathBuf, Off)> {
    link_to_network(path, mounts, read_if_link)
}

/// Why a path is not touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Off {
    /// It reaches a server.
    Network,
    /// It can't be checked by its text: a volume or device name (`\\?\Volume{…}`, `\\.\pipe`), a
    /// `..` in a `\\?\` or `\??\` path, a link that can't be read, or a chain past the cap. A git
    /// dir there refuses the launch.
    Unknown,
}

/// Where a path is, by [`classify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Place {
    /// On a local disk: the path, with any `subst` drive replaced by what it maps to.
    Local(PathBuf),
    /// Not to be touched.
    Off(Off),
}

/// Where `path` is by its text and `mounts`, with no file call. `\\?\` and `\??\` are read
/// alike, and a `..` in either is unknown (the kernel takes it as written, after any link before
/// it, so it can't be taken off by text): `UNC\…` after either is network, a drive letter goes
/// on to the drive check, anything else (a volume or device name) is unknown. A `\\.\` device
/// path with a `..` is unknown too (Win32 takes it off above the drive letter, whose root is
/// `\\.\`); without one it is `UNC\…` (network), a drive letter (`\\.\D:\x` is `D:\x`), or
/// another device (unknown). Other paths
/// starting with two separators (`\\host\share`, `//host/share`) are network. A drive letter
/// is what its device is ([`Drive`]), a `subst` drive one mapping at a time (up to
/// [`SUBST_MAX`], past which it is unknown). Any other path is network if its longest
/// mount-point prefix is.
pub(crate) fn classify(path: &Path, mounts: &Mounts) -> Place {
    let mut current = path.to_owned();
    for _ in 0..=SUBST_MAX {
        let bytes = current.as_os_str().as_encoded_bytes();
        let sep = |b: &u8| *b == b'/' || *b == b'\\';
        let mut rest = bytes;
        if let Some(tail) = bytes
            .strip_prefix(br"\\?\")
            .or_else(|| bytes.strip_prefix(br"\??\"))
        {
            if tail.split(sep).any(|name| name == b"..") {
                return Place::Off(Off::Unknown);
            }
            if tail
                .get(..4)
                .is_some_and(|head| head.eq_ignore_ascii_case(br"UNC\"))
            {
                return Place::Off(Off::Network);
            }
            if drive_letter(tail).is_none() {
                return Place::Off(Off::Unknown);
            }
            rest = tail;
        } else if bytes.get(..2).is_some_and(|two| two.iter().all(sep)) {
            let device = bytes.get(2) == Some(&b'.') && bytes.get(3).is_some_and(sep);
            let Some(tail) = bytes.get(4..).filter(|_| device) else {
                return Place::Off(Off::Network);
            };
            // Win32's root for a device path is `\\.\` itself, so a `..` climbs above the drive
            // letter (`\\.\D:\..\Z:\m` opens `Z:\m`): unknown, as in a `\\?\` path.
            if tail.split(sep).any(|name| name == b"..") {
                return Place::Off(Off::Unknown);
            }
            if let [u, n, c, after, ..] = tail
                && [*u, *n, *c].eq_ignore_ascii_case(b"UNC")
                && sep(after)
            {
                return Place::Off(Off::Network);
            }
            if drive_letter(tail).is_none() {
                return Place::Off(Off::Unknown);
            }
            let Some(drive_path) = current.to_str().and_then(|text| text.get(4..)) else {
                return Place::Off(Off::Unknown);
            };
            current = PathBuf::from(drive_path);
            continue;
        }
        let Some(letter) = drive_letter(rest) else {
            return by_mount(current, mounts);
        };
        let target = match mounts.drive(letter) {
            Drive::Local => return Place::Local(current),
            Drive::Network => return Place::Off(Off::Network),
            Drive::Subst(target) => target,
        };
        let skip = bytes.len() - rest.len() + 2;
        let Some(after) = current.to_str().and_then(|text| text.get(skip..)) else {
            return Place::Off(Off::Unknown);
        };
        let mut next = target.clone();
        let seps: &[char] = &['\\', '/'];
        let after = if next.ends_with(seps) {
            after.trim_start_matches(seps)
        } else {
            after
        };
        if !after.is_empty() && !after.starts_with(seps) && !next.ends_with(seps) {
            next.push('\\');
        }
        next.push_str(after);
        current = PathBuf::from(next);
    }
    Place::Off(Off::Unknown)
}

/// `path` by its longest mount-point prefix.
fn by_mount(path: PathBuf, mounts: &Mounts) -> Place {
    let network = mounts
        .points
        .iter()
        .filter(|(point, _)| under(&path, point))
        .max_by_key(|(point, _)| point.components().count())
        .is_some_and(|(_, network)| *network);
    if network {
        Place::Off(Off::Network)
    } else {
        Place::Local(path)
    }
}

/// Whether `path` is not to be touched by [`classify`]: network or unknown.
pub(crate) fn is_network_path(path: &Path, mounts: &Mounts) -> bool {
    matches!(classify(path, mounts), Place::Off(_))
}

/// The drive letter `bytes` starts with (`X:`), if any.
fn drive_letter(bytes: &[u8]) -> Option<u8> {
    match bytes {
        [letter, b':', ..] if letter.is_ascii_alphabetic() => Some(*letter),
        _ => None,
    }
}

/// Where `path` leaves the local disks, name by name and link by link, if it does: the path as
/// a whole, each name before it is looked at, and each link's target as written before it is
/// followed, by [`classify`]. `read` is the one file call, `symlink_metadata` then `read_link`
/// on a local path: its target if a link, `None` if not. A missing name ends the chain (local:
/// resolving it then fails without leaving the disk). Any other failed read, and a chain past
/// [`HOPS_MAX`] links, is unknown. On Windows `..` is taken off by text, as Win32 does before
/// it opens a path and the I/O manager does with a relative link's target joined to its folder;
/// elsewhere a `..` steps back from wherever the links led, as the kernel does.
pub(crate) fn link_to_network(
    path: &Path,
    mounts: &Mounts,
    read: impl Fn(&Path) -> io::Result<Option<PathBuf>>,
) -> Option<(PathBuf, Off)> {
    link_to_network_as(path, mounts, read, cfg!(windows))
}

/// [`link_to_network`], taking `..` off by text when `dots_by_text` (Windows's way).
pub(crate) fn link_to_network_as(
    path: &Path,
    mounts: &Mounts,
    read: impl Fn(&Path) -> io::Result<Option<PathBuf>>,
    dots_by_text: bool,
) -> Option<(PathBuf, Off)> {
    // As written first: a `..` in a `\\?\` or `\??\` path is unknown before `lexical` drops it.
    if let Place::Off(off) = classify(path, mounts) {
        return Some((path.to_owned(), off));
    }
    let path = if dots_by_text {
        lexical(path)
    } else {
        path.to_owned()
    };
    let start = match classify(&path, mounts) {
        Place::Off(off) => return Some((path, off)),
        Place::Local(start) => start,
    };
    let mut todo: VecDeque<Part> = start.components().map(Part::of).collect();
    let mut done = PathBuf::new();
    let mut hops = 0;
    while let Some(part) = todo.pop_front() {
        let name = match part {
            Part::Fixed(fixed) => {
                done.push(fixed);
                continue;
            }
            Part::Up => {
                done.pop();
                continue;
            }
            Part::Here => continue,
            Part::Name(name) => name,
        };
        let candidate = done.join(name);
        let next = match classify(&candidate, mounts) {
            Place::Off(off) => return Some((candidate, off)),
            Place::Local(next) => next,
        };
        let target = match read(&next) {
            Ok(Some(target)) => target,
            Ok(None) => {
                done = next;
                continue;
            }
            Err(error) if ends_chain(&error) => return None,
            Err(_) => return Some((next, Off::Unknown)),
        };
        let target = match classify(&target, mounts) {
            Place::Off(off) => return Some((target, off)),
            Place::Local(target) => target,
        };
        hops += 1;
        if hops > HOPS_MAX {
            return Some((next, Off::Unknown));
        }
        if dots_by_text {
            // The target joined to the link's folder, then the rest, with `..` taken off by text.
            let folder = next.parent().unwrap_or(&done);
            let mut whole = folder.join(target);
            for part in todo.drain(..) {
                match part {
                    Part::Fixed(name) | Part::Name(name) => whole.push(name),
                    Part::Up => whole.push(".."),
                    Part::Here => {}
                }
            }
            let whole = lexical(&whole);
            let whole = match classify(&whole, mounts) {
                Place::Off(off) => return Some((whole, off)),
                Place::Local(whole) => whole,
            };
            todo = whole.components().map(Part::of).collect();
            done = PathBuf::new();
        } else {
            for part in target.components().rev() {
                todo.push_front(Part::of(part));
            }
        }
    }
    None
}

/// Whether a failed look at a name means the name isn't there, which ends a chain as local.
fn ends_chain(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// One step of [`link_to_network`]'s path: a prefix or root, kept as it is; `..`; `.`; a name.
enum Part {
    Fixed(OsString),
    Up,
    Here,
    Name(OsString),
}

impl Part {
    fn of(part: Component<'_>) -> Part {
        match part {
            Component::Prefix(_) | Component::RootDir => Part::Fixed(part.as_os_str().to_owned()),
            Component::CurDir => Part::Here,
            Component::ParentDir => Part::Up,
            Component::Normal(name) => Part::Name(name.to_owned()),
        }
    }
}

/// [`link_to_network`]'s file call: the target of the link at `path`, `None` if not a link.
pub(crate) fn read_if_link(path: &Path) -> io::Result<Option<PathBuf>> {
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        std::fs::read_link(path).map(Some)
    } else {
        Ok(None)
    }
}

/// `path` with `.` dropped and each `..` taking off the name before it, by text alone, as
/// Win32 does: never past the root, and kept at the start of a relative path.
pub(crate) fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                None | Some(Component::ParentDir) => out.push(".."),
                Some(Component::Prefix(_) | Component::RootDir | Component::CurDir) => {}
            },
            other @ (Component::Prefix(_) | Component::RootDir | Component::Normal(_)) => {
                out.push(other);
            }
        }
    }
    out
}

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "FFI to read the mount table with getfsstat")]
mod os {
    use std::ffi::CStr;
    use std::io;
    use std::path::PathBuf;

    use super::Mounts;

    pub(super) fn current() -> Mounts {
        read().unwrap_or_else(|error| {
            tracing::warn!(%error, "the matter walk could not read the mount list");
            Mounts {
                unread: true,
                ..Mounts::default()
            }
        })
    }

    fn read() -> io::Result<Mounts> {
        let list = mounts()?;
        let text = |name: &[libc::c_char]| {
            let bytes: Vec<u8> = name.iter().map(|c| c.to_ne_bytes()[0]).collect();
            CStr::from_bytes_until_nul(&bytes)
                .map(|text| text.to_string_lossy().into_owned())
                .map_err(io::Error::other)
        };
        let mut points = Vec::with_capacity(list.len());
        for mount in &list {
            let kind = text(&mount.f_fstypename)?;
            points.push((
                PathBuf::from(text(&mount.f_mntonname)?),
                Mounts::is_network_fs(&kind),
            ));
        }
        Ok(Mounts {
            points,
            drives: Vec::new(),
            unread: false,
        })
    }

    /// The kernel's mount list, asked again with more room while it comes back full.
    fn mounts() -> io::Result<Vec<libc::statfs>> {
        // SAFETY: a null buffer asks only for the count of mounts.
        let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        let mut room = usize::try_from(count).map_err(|_negative| io::Error::last_os_error())? + 8;
        for _ in 0..8 {
            // SAFETY: statfs is plain old data, for which all zeroes is a valid value.
            let mut list: Vec<libc::statfs> = vec![unsafe { std::mem::zeroed() }; room];
            let bytes =
                i32::try_from(room * size_of::<libc::statfs>()).map_err(io::Error::other)?;
            // SAFETY: `list` holds `bytes` bytes of writable statfs records.
            let got = unsafe { libc::getfsstat(list.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
            let got = usize::try_from(got).map_err(|_negative| io::Error::last_os_error())?;
            if got < room {
                list.truncate(got);
                return Ok(list);
            }
            room *= 2;
        }
        Err(io::Error::other("the mount list kept growing"))
    }
}

#[cfg(windows)]
mod os {
    use super::Mounts;

    pub(super) fn current() -> Mounts {
        Mounts {
            points: Vec::new(),
            drives: super::drives::os::table(),
            unread: false,
        }
    }
}

/// Linux and the rest: no mount table (decision 0049: not supported for this check).
#[cfg(not(any(target_os = "macos", windows)))]
mod os {
    use super::Mounts;

    pub(super) fn current() -> Mounts {
        Mounts::default()
    }
}

#[cfg(test)]
mod tests;
