//! Where a path the agent wrote really leads, for the data folder fence: `~` and `~user` made
//! absolute, `..` taken on what is resolved so far, links followed even when their target is
//! missing, walked by the shared network-safe walk ([`crate::network_paths::resolve::walk`]). A
//! path that is or leads to a network location, or that the walk cannot vouch for (fail-closed),
//! counts as inside the data folder, so it is refused, unless it lies by its text in a matter
//! folder the lawyer keeps on a share ([`under_share_matter`]): that one is outside and untouched.

use std::path::{Component, Path, PathBuf};

pub(crate) use crate::network_paths::resolve::{Disk, drive_form};
use crate::network_paths::resolve::{Walked, walk as walk_on};
use crate::network_paths::{is_url, under_share_matter};

/// Where one path leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Found {
    /// A local path, resolved.
    Path(PathBuf),
    /// A network location on the way (found before touching it), or anything the walk cannot
    /// vouch for ([`Walked::Refused`]): counts as inside the data folder.
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
    let path = path.to_owned();
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

/// Where the absolute `start` leads: untouched when it lies by its text in the share matter
/// `matter`; else walked on `disk` ([`crate::network_paths::resolve::walk`]), a path it refuses
/// counting as inside the data folder.
fn walk(start: &Path, matter: &Path, disk: &Disk<'_>) -> Found {
    if under_share_matter(start, matter, disk.net) {
        return Found::ShareMatter;
    }
    match walk_on(start, disk) {
        Walked::Local(path) => Found::Path(path),
        Walked::Refused => Found::Refused,
    }
}
