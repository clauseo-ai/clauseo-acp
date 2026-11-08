//! Where a path the agent wrote really leads, for the data folder fence: `~` and `~user` made
//! absolute, `..` taken on what is resolved so far, links followed even when their target is
//! missing, and a Windows path to the local machine's own share put in its drive form. A network
//! location is never touched: the path, each name on the way and each link's target are checked
//! by their text ([`NetEnv::is_network`]) before anything looks at them, and a path that is or
//! leads to one counts as inside the data folder, so it is refused, unless it lies by its text in
//! a matter folder the lawyer keeps on a share ([`under_share_matter`]): that one is outside and
//! untouched.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::network_paths::resolve::{Entry, Fs, RealFs};
use crate::network_paths::{NetEnv, is_url, under_share_matter};

/// The most links one path may pass through before it counts as a loop (Linux's own limit).
const MAX_HOPS: usize = 40;

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
    /// The real disk under `net`.
    pub(crate) fn real(net: &'a NetEnv) -> Self {
        Self {
            fs: &RealFs,
            real: &canonical,
            net,
        }
    }
}

/// `path` canonicalised (`dunce`), asked only of a local name that is not a link.
fn canonical(path: &Path) -> Option<PathBuf> {
    dunce::canonicalize(path).ok()
}

/// Where one path leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Found {
    /// A local path, resolved.
    Path(PathBuf),
    /// A link loop, a chain of more than [`MAX_HOPS`] links, or a network location on the way
    /// (found before touching it): counts as inside the data folder.
    Refused,
    /// By its text in the matter folder the lawyer keeps on a network share: outside the data
    /// folder, and never touched.
    ShareMatter,
}

/// Where `path` may lead: one place, or for a `~user` path whose user is not known, one under
/// each known home (`home` itself and `home`'s parent joined with the name). A relative path is
/// taken from `cwd` (the matter folder); `~` and `~/…` from `home`. A `~user` path is never
/// taken from `cwd`; with no known home it leads nowhere (an empty list).
pub(crate) fn resolve_all(
    path: &Path,
    cwd: &Path,
    home: Option<&Path>,
    disk: &Disk<'_>,
) -> Vec<Found> {
    // The agent's text itself, before it is joined to anything: a network form is found as
    // written (a UNC path is relative on Unix, and would be joined to `cwd`). Not a URL: the
    // file system reads `https://host/a` as a relative path, joined and walked as any other
    // (a shell command's URL argument is no path the fence would touch on a server).
    if under_share_matter(path, cwd, disk.net) {
        return vec![Found::ShareMatter];
    }
    if disk.net.is_network(path) && !is_url(&path.to_string_lossy()) {
        return vec![Found::Refused];
    }
    let path = if cfg!(windows) {
        path.to_str()
            .and_then(drive_form)
            .map_or_else(|| path.to_owned(), PathBuf::from)
    } else {
        path.to_owned()
    };
    let mut parts = path.components();
    let first = parts.clone().next();
    let tilde = match first {
        Some(Component::Normal(name)) => name.to_str().and_then(|name| name.strip_prefix('~')),
        _ => None,
    };
    let starts: Vec<PathBuf> = match tilde {
        None if path.is_absolute() => vec![path.clone()],
        None => vec![cwd.join(&path)],
        Some(user) => {
            parts.next();
            let rest: PathBuf = parts.collect();
            homes_of(user, home)
                .into_iter()
                .map(|home| home.join(&rest))
                .collect()
        }
    };
    starts.iter().map(|start| walk(start, cwd, disk)).collect()
}

/// The one place `path` leads ([`resolve_all`]'s first), [`Found::Refused`] when none.
pub(crate) fn resolve(path: &Path, cwd: &Path, home: Option<&Path>, disk: &Disk<'_>) -> Found {
    resolve_all(path, cwd, home, disk)
        .into_iter()
        .next()
        .unwrap_or(Found::Refused)
}

/// The homes `~user` may name: the lawyer's `home` for `~`; for a named user, the user's home
/// from the system's user database on Unix, or, when it is not known there (and on Windows),
/// each known home: `home` and its parent joined with the name.
fn homes_of(user: &str, home: Option<&Path>) -> Vec<PathBuf> {
    if user.is_empty() {
        return home.map(Path::to_owned).into_iter().collect();
    }
    #[cfg(unix)]
    if let Ok(Some(found)) = nix::unistd::User::from_name(user) {
        return vec![found.dir];
    }
    let Some(home) = home else {
        return Vec::new();
    };
    let mut homes = vec![home.to_owned()];
    homes.extend(home.parent().map(|parent| parent.join(user)));
    homes
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

/// The absolute `path` walked part by part: a path in the share matter `matter` left untouched;
/// a network location (the path, a name on the way, a link's target) refused before it is
/// looked at; each local name that is not a link put in its canonical form; a link read (never
/// followed by the system) and its target walked in its place, a relative target from the
/// link's own folder; a name that can't be looked at (missing) kept as written. Refused past
/// [`MAX_HOPS`] links.
fn walk(path: &Path, matter: &Path, disk: &Disk<'_>) -> Found {
    let net = disk.net;
    if under_share_matter(path, matter, net) {
        return Found::ShareMatter;
    }
    if net.is_network(path) {
        return Found::Refused;
    }
    let mut at = PathBuf::new();
    let mut rest = parts_of(path);
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
                    return Found::Refused;
                }
                match disk.fs.entry(&next) {
                    Ok(Entry::Link) => {
                        hops = hops.saturating_add(1);
                        if hops > MAX_HOPS {
                            return Found::Refused;
                        }
                        let Ok(target) = disk.fs.read_link(&next) else {
                            at = next;
                            continue;
                        };
                        let target = net.plain(&local(target));
                        if net.is_network(&target) || net.is_network(&at.join(&target)) {
                            return Found::Refused;
                        }
                        if target.has_root() {
                            at = PathBuf::new();
                        }
                        rest.extend(parts_of(&target));
                    }
                    Ok(Entry::Dir | Entry::File) => {
                        at = (disk.real)(&next).map_or(next, local);
                        if net.is_network(&at) {
                            return Found::Refused;
                        }
                    }
                    Err(_) => at = next,
                }
            }
        }
    }
    Found::Path(at)
}

/// `path` in its drive form on Windows when it names the local machine's own share
/// ([`drive_form`]); as it is elsewhere.
fn local(path: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return path;
    }
    path.to_str()
        .and_then(drive_form)
        .map_or(path, PathBuf::from)
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
