//! Where a path the agent wrote really leads, for the data folder fence: `~` and `~user` made
//! absolute, `..` taken on what is resolved so far, links followed even when their target is
//! missing, and a Windows path to the local machine's own share put in its drive form.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// The most links one path may pass through before it counts as a loop (Linux's own limit).
const MAX_HOPS: usize = 40;

/// Where `path` may lead: one place, or for a `~user` path whose user is not known, one under
/// each known home (`home` itself and `home`'s parent joined with the name). `None` stands for
/// a loop or a chain of more than [`MAX_HOPS`] links, which the fence treats as inside the data
/// folder. A relative path is taken from `cwd`; `~` and `~/…` from `home`. A `~user` path is
/// never taken from `cwd`; with no known home it leads nowhere (an empty list).
pub(crate) fn resolve_all(path: &Path, cwd: &Path, home: Option<&Path>) -> Vec<Option<PathBuf>> {
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
    starts.iter().map(|start| walk(start)).collect()
}

/// The one place `path` leads ([`resolve_all`]'s first), or `None` for a loop.
pub(crate) fn resolve(path: &Path, cwd: &Path, home: Option<&Path>) -> Option<PathBuf> {
    resolve_all(path, cwd, home).into_iter().next().flatten()
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

/// The absolute `path` walked part by part: each prefix that exists canonicalised; a link that
/// does not resolve (its target missing, or a loop) read and its target walked in its place, a
/// relative target from the link's own folder; a missing name kept as written. `None` past
/// [`MAX_HOPS`] links.
fn walk(path: &Path) -> Option<PathBuf> {
    let mut at = PathBuf::new();
    let mut rest = parts_of(path);
    let mut hops = 0;
    while let Some(part) = rest.pop() {
        match part {
            Part::Root(root) => at.push(root),
            Part::Up => {
                at.pop();
            }
            Part::Name(name) => {
                let next = at.join(&name);
                if let Ok(real) = dunce::canonicalize(&next) {
                    at = local(real);
                } else if let Ok(target) = std::fs::read_link(&next) {
                    hops += 1;
                    if hops > MAX_HOPS {
                        return None;
                    }
                    let target = local(target);
                    if target.has_root() {
                        at = PathBuf::new();
                    }
                    rest.extend(parts_of(&target));
                } else {
                    at = next;
                }
            }
        }
    }
    Some(at)
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
