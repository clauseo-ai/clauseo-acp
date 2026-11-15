//! The full 8.3 short form (`GetShortPathNameW`, `CLAUSE~1`) of a path a fence denies, beside
//! its long form: the one way both fences add it, the other matters' (`agent::matter_fence`)
//! and the data folder's spellings (`actor::turn::launch_checks::folders`). Claude Code matches
//! its deny rules against the path as the tool was given it, never spelt out, so a Read through
//! `C:\Users\CLAUSE~2\Matters\B` passed a rule naming `C:\Users\clauseo-lawyer\Matters\B`.
//! Only the whole short form is added: a path with some names short and some long is in no rule,
//! the known gap until the file tools' hook spells every path out before it runs.

use std::path::{Path, PathBuf};

use crate::network_paths::NetEnv;

/// `paths`, each followed by its full short form when it has one that differs ([`short_form`]);
/// none added twice. A network path is never asked (a share can hang the call); off Windows,
/// and on a volume that makes no short names (the call gives the path back), nothing is added.
/// On Windows a short form that can't be read is counted in one warning, naming no path.
pub(crate) fn with_short_forms(paths: Vec<PathBuf>, net: &NetEnv) -> Vec<PathBuf> {
    with_short_forms_by(paths, net, &short_form)
}

/// [`with_short_forms`] asking `short` for a path's short form.
pub(crate) fn with_short_forms_by(
    paths: Vec<PathBuf>,
    net: &NetEnv,
    short: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::with_capacity(paths.len());
    let mut unread = 0_usize;
    for path in paths {
        let found = if net.is_network(&path) {
            None
        } else {
            let asked = short(&path);
            unread += usize::from(asked.is_none());
            asked.filter(|found| *found != path)
        };
        for path in std::iter::once(path).chain(found) {
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    if unread > 0 && net.windows {
        tracing::warn!(
            unread,
            "short forms could not be read; those paths go long alone"
        );
    }
    out
}

/// The full short form of the local path `path` (`GetShortPathNameW`); `None` off Windows, for
/// a path that does not exist, or when it can't be read.
pub(crate) fn short_form(path: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        crate::network_paths::short_path(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

#[cfg(test)]
#[path = "short_forms_tests.rs"]
mod tests;
