//! Where a path an agent named really leads, without ever touching a network location: walked
//! name by name, each step checked by [`NetEnv::is_network`] before it is looked at, each link
//! read (never followed by the system) and its target checked the same way before the walk goes
//! on into it. Fail-closed: anything the walk cannot vouch for (a link it can't read, an error
//! other than a missing name, a loop or more than [`MAX_HOPS`] links, a relative path) is
//! [`Walked::Refused`], so a caller never hands it to the file system. Every "may this agent
//! path be touched?" decision uses it: the data fence, its held-open check, the after-turn tool
//! paths and the pre-write guard. `std::fs::canonicalize` and `dunce::canonicalize` would
//! follow a local link to `/net/host/…` or a Windows link to a UNC target and sign in there, so
//! neither is used on an agent's path before this walk vouches for it.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};

use super::NetEnv;
use super::no_follow::{self, Access, NoFollow};

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod windows_tests;

/// The most links one path may pass through; past it the walk refuses (fail-closed: Windows
/// itself follows up to 63).
pub(crate) const MAX_HOPS: usize = 40;

/// What one name on the disk is, the link itself rather than its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entry {
    Dir,
    File,
    Link,
}

/// The two looks the walk takes at the disk, so a test can see each one.
pub(crate) trait Fs {
    /// What `path` is, never following a link at it (`symlink_metadata`).
    fn entry(&self, path: &Path) -> io::Result<Entry>;
    /// The target a link at `path` names (`read_link`), which the system does not follow.
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
    /// The identity of the local file or folder at `path` (its device or volume and its file
    /// number), never following a link at it: asked only of a path the walk vouched for. An
    /// error when it can't be read (`NotFound` when there is no such name).
    fn identity(&self, path: &Path) -> io::Result<(u64, u128)>;
}

/// The real disk. Every look goes through the folder above the name, opened with no link
/// followed on its way ([`super::no_follow`]), and then looks at the name itself without
/// following it: a folder the walk already passed and an agent swapped for a link since is
/// refused ([`super::no_follow::LinkOnTheWay`], an error the walk refuses on), never followed.
/// On Windows a name's kind and reparse tag come from its folder's listing, so a reparse point
/// that is not a known local kind is never opened.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RealFs;

/// The folder above `path` opened with no link followed, and `path`'s last name; `None` for a
/// root, which has no folder above it.
fn folder_and_name(path: &Path) -> io::Result<Option<(NoFollow, &OsStr)>> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(None);
    };
    let folder = no_follow::open_path(parent, Access::Folder).inspect_err(log_link)?;
    Ok(Some((folder, name)))
}

/// Logs a link found on a walked folder (the walk refuses on it), by its kind only.
fn log_link(error: &io::Error) {
    if no_follow::is_link_on_the_way(error) {
        no_follow::log_refusal(error);
    }
}

impl Fs for RealFs {
    fn entry(&self, path: &Path) -> io::Result<Entry> {
        match folder_and_name(path)? {
            Some((folder, name)) => folder.entry_at(name),
            None => Ok(Entry::Dir),
        }
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        match folder_and_name(path)? {
            Some((folder, name)) => folder.read_link_at(name),
            None => Err(io::ErrorKind::InvalidInput.into()),
        }
    }

    fn identity(&self, path: &Path) -> io::Result<(u64, u128)> {
        match folder_and_name(path)? {
            Some((folder, name)) => folder.identity_at(name),
            None => no_follow::open_path(path, Access::Look)?.identity(),
        }
    }
}

/// How the walk looks at the disk, and what counts as a network location.
pub(crate) struct Disk<'a> {
    /// What a name is and where a link points, never following one.
    pub(crate) fs: &'a dyn Fs,
    /// The canonical path of a local name that is not a link (its long name and its case as on
    /// the disk), when it has one.
    pub(crate) real: &'a dyn Fn(&Path) -> Option<PathBuf>,
    /// The network rules and this system's remote mounts and drives.
    pub(crate) net: &'a NetEnv,
}

impl<'a> Disk<'a> {
    /// The real disk under `net`, each local name put in its canonical form (the data fence,
    /// which compares the resolved path).
    pub(crate) fn real(net: &'a NetEnv) -> Self {
        Self {
            fs: &RealFs,
            real: &canonical,
            net,
        }
    }

    /// The real disk under `net`, for a caller that goes on to use the walked path
    /// ([`super::touchable`]) and compares it with the matter folder's canonical path. The same
    /// as [`Self::real`]: on Windows each local name in its canonical form (its long name, not
    /// its 8.3 alias); as written elsewhere, where the walk's result is already canonical but for
    /// case and Unicode form, which every comparison folds as the file system does.
    pub(crate) fn resolved(net: &'a NetEnv) -> Self {
        Self::real(net)
    }
}

/// The canonical form of a local name that is not a link, once every name before it was
/// vouched for: on Windows the system's final path for it, opened with no link followed (a link
/// swapped in refuses, and the name stays as written); elsewhere the name as written (`None`):
/// with every link already read and `..` taken, the walked path is the canonical one but for
/// case and Unicode form on macOS, which every comparison folds.
fn canonical(path: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        no_follow::open_path(path, Access::Look)
            .inspect_err(log_link)
            .ok()?
            .final_path()
            .ok()
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// Where a path leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Walked {
    /// A local path, every name on the way looked at (or missing) and every link's target
    /// checked; resolved.
    Local(PathBuf),
    /// A network location on the way (found before touching it), or anything the walk cannot
    /// vouch for: never to be handed to the file system.
    Refused,
}

/// Where the absolute `path` leads on `disk`: each local name that is not a link put in its
/// canonical form; a link read and its target walked in its place, a relative target from the
/// link's own folder; a missing name kept as written, and every name after it still looked at
/// (so a `..` back out of it reaches names that are checked: `C:\m\missing\..\link` checks
/// `link`); `..` taken on what is resolved so far. [`Walked::Refused`] for a network location
/// (the path, a name on the way, a link's target), a relative path, a link that can't be read, an
/// error other than a missing name, or more than [`MAX_HOPS`] links.
///
/// With Windows' rules (`net.windows`) `.` and `..` go first, by text, as Win32 drops them
/// before anything opens the path: `C:\m\j\..\x` opens `C:\m\x` whatever `j` is, so the walk
/// never backs up from where a junction led. A link's relative target is joined to the link's
/// folder by text too and its `..` dropped the same way, and a drive-relative target (`\x`,
/// `C:x`) is refused. Unix keeps the kernel's way: `..` backs up from where a link led.
pub(crate) fn walk(path: &Path, disk: &Disk<'_>) -> Walked {
    let net = disk.net;
    if net.is_network(path) {
        return Walked::Refused;
    }
    let path = net.plain(path);
    if !path.is_absolute() {
        return Walked::Refused;
    }
    let path = if net.windows { lexical(&path) } else { path };
    if net.is_network(&path) {
        return Walked::Refused;
    }
    let mut at = PathBuf::new();
    let mut rest = parts_of(&path);
    let mut hops = 0_usize;
    while let Some(part) = rest.pop() {
        match part {
            Part::Root(root) => at.push(root),
            Part::Up => {
                at.pop();
            }
            Part::Name(name) => {
                let next = at.join(&name);
                if net.is_network(&next) {
                    return Walked::Refused;
                }
                match disk.fs.entry(&next) {
                    Ok(Entry::Link) => {
                        hops = hops.saturating_add(1);
                        if hops > MAX_HOPS {
                            return Walked::Refused;
                        }
                        let Ok(target) = disk.fs.read_link(&next) else {
                            return Walked::Refused;
                        };
                        let target = net.plain(&target);
                        if net.is_network(&target) || net.is_network(&at.join(&target)) {
                            return Walked::Refused;
                        }
                        let prefixed = target
                            .components()
                            .any(|part| matches!(part, Component::Prefix(_)));
                        if !target.is_absolute() && (target.has_root() || prefixed) {
                            // Drive-relative (`\x`, `C:x`): no folder it is taken from.
                            return Walked::Refused;
                        }
                        if net.windows {
                            // Windows reads a link's target by its text too: joined to the
                            // link's folder, `..` dropped, then walked whole from its root.
                            let joined = lexical(&at.join(&target));
                            if net.is_network(&joined) {
                                return Walked::Refused;
                            }
                            at = PathBuf::new();
                            rest.extend(parts_of(&joined));
                        } else {
                            // The kernel takes a relative target from the link's folder, `..`
                            // on what is resolved: its parts go on from `at`.
                            if target.is_absolute() {
                                at = PathBuf::new();
                            }
                            rest.extend(parts_of(&target));
                        }
                    }
                    Ok(Entry::Dir | Entry::File) => {
                        at = (disk.real)(&next).unwrap_or(next);
                        if net.is_network(&at) {
                            return Walked::Refused;
                        }
                    }
                    Err(error) if no_such_name(&error) => at = next,
                    Err(_) => return Walked::Refused,
                }
            }
        }
    }
    Walked::Local(at)
}

/// Whether `error` says the name is not there or can't be one: `NotFound`, or a name the system
/// rejects (`InvalidFilename`; on Windows `ERROR_INVALID_NAME` and `ERROR_BAD_PATHNAME`, which a
/// shell word with `*` or `?` or a URL's `:` gets). Nothing past such a name is opened, so the
/// walk keeps it as written and still checks every later name.
pub(crate) fn no_such_name(error: &io::Error) -> bool {
    /// `ERROR_INVALID_NAME` and `ERROR_BAD_PATHNAME` (`winerror.h`).
    const WINDOWS_BAD_NAME: [i32; 2] = [123, 161];
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::InvalidFilename
    ) || (cfg!(windows)
        && error
            .raw_os_error()
            .is_some_and(|code| WINDOWS_BAD_NAME.contains(&code)))
}

/// One part of a path still to walk.
enum Part {
    /// A drive prefix or the root: the walk starts over from it.
    Root(OsString),
    /// `..`.
    Up,
    /// A name.
    Name(OsString),
}

/// `path` with `.` and `..` taken off by their text, as Win32 path parsing does (`..` never
/// past the root).
pub(crate) fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                out.push(part.as_os_str());
            }
        }
    }
    out
}

/// `path`'s parts, last first, to pop in order.
fn parts_of(path: &Path) -> Vec<Part> {
    let mut parts: Vec<Part> = path
        .components()
        .filter_map(|part| match part {
            Component::Prefix(_) | Component::RootDir => Some(Part::Root(part.as_os_str().into())),
            Component::CurDir => None,
            Component::ParentDir => Some(Part::Up),
            Component::Normal(name) => Some(Part::Name(name.into())),
        })
        .collect();
    parts.reverse();
    parts
}

/// A Windows path to this machine's own drive in its drive form: `\\localhost\c$\x`,
/// `\\127.0.0.1\c$\x`, `\\?\UNC\localhost\c$\x`, `\\.\UNC\localhost\c$\x`, `\\?\C:\x` and
/// `\\.\C:\x` all become `C:\x`, `/` read as `\`, names in any case. `None` for any other path.
/// Pure text, so it is tested on every system.
pub(crate) fn drive_form(text: &str) -> Option<String> {
    let text = text.replace('/', "\\");
    let lower = text.to_ascii_lowercase();
    // ASCII folding keeps every byte where it was, so a prefix of `lower` is one of `text`.
    let strip = |prefix: &str| {
        lower
            .starts_with(prefix)
            .then(|| text.get(prefix.len()..))
            .flatten()
    };
    let (drive, rest) = if let Some(rest) = strip(r"\\?\unc\").or_else(|| strip(r"\\.\unc\")) {
        share(rest)?
    } else if let Some(rest) = strip(r"\\?\").or_else(|| strip(r"\\.\")) {
        let mut chars = rest.chars();
        let drive = chars.next().filter(char::is_ascii_alphabetic)?;
        (drive, chars.as_str().strip_prefix(':')?)
    } else {
        share(strip(r"\\")?)?
    };
    let rest = rest.trim_start_matches('\\');
    Some(format!("{}:\\{rest}", drive.to_ascii_uppercase()))
}

/// `localhost\c$\rest` (or `127.0.0.1`, `[::1]`): the drive letter of the share and the rest.
fn share(text: &str) -> Option<(char, &str)> {
    let (host, rest) = text.split_once('\\')?;
    let local = ["localhost", "127.0.0.1", "[::1]", "::1"];
    if !local.iter().any(|name| host.eq_ignore_ascii_case(name)) {
        return None;
    }
    let (share, rest) = rest.split_once('\\').unwrap_or((rest, ""));
    let mut chars = share.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(drive), Some('$'), None) if drive.is_ascii_alphabetic() => Some((drive, rest)),
        _ => None,
    }
}
