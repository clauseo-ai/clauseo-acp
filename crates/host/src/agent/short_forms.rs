//! The full 8.3 short form (`GetShortPathNameW`, `CLAUSE~1`) of a path a fence denies, beside
//! its long form: the one way both fences add it, the other matters' (`agent::matter_fence`)
//! and the data folder's spellings (`actor::turn::launch_checks::folders`). Claude Code matches
//! its deny rules against the path as the tool was given it, never spelt out, so a Read through
//! `C:\Users\CLAUSE~2\Matters\B` passed a rule naming `C:\Users\clauseo-lawyer\Matters\B`.
//! Only the whole short form is added: a path with some names short and some long is in no rule,
//! the known gap until the file tools' hook spells every path out before it runs.

use std::path::{Path, PathBuf};

use crate::network_paths::NetEnv;

/// What the short-form lookup found for one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Looked {
    /// Its full short form: the path itself where it has none (a volume that makes no short
    /// names, or off Windows).
    Form(PathBuf),
    /// No such path (a registered matter whose folder is gone): nothing to add, nothing wrong.
    #[cfg_attr(
        all(not(windows), not(test)),
        expect(dead_code, reason = "only Windows' lookup finds a path missing")
    )]
    Missing,
    /// It exists, but its short form could not be read.
    #[cfg_attr(
        all(not(windows), not(test)),
        expect(dead_code, reason = "only Windows' lookup fails to read one")
    )]
    Unread,
}

/// `paths`, each followed by its full short form when it has one that differs ([`short_form`]);
/// none added twice. A network path is never asked (a share can hang the call); off Windows,
/// and on a volume that makes no short names (the call gives the path back), nothing is added.
/// On Windows a short form that can't be read for a path that exists is counted in one
/// warning, naming no path; a missing one is not.
pub(crate) fn with_short_forms(paths: Vec<PathBuf>, net: &NetEnv) -> Vec<PathBuf> {
    let (out, unread) = short_forms_counted(paths, net, &short_form);
    if unread > 0 && net.windows {
        tracing::warn!(
            unread,
            "short forms could not be read; those paths go long alone"
        );
    }
    out
}

/// [`with_short_forms`] asking `short` for each path's short form, with how many it could not
/// read.
pub(crate) fn short_forms_counted(
    paths: Vec<PathBuf>,
    net: &NetEnv,
    short: &dyn Fn(&Path) -> Looked,
) -> (Vec<PathBuf>, usize) {
    let mut out: Vec<PathBuf> = Vec::with_capacity(paths.len());
    let mut unread = 0_usize;
    for path in paths {
        let found = if net.is_network(&path) {
            None
        } else {
            match short(&path) {
                Looked::Form(found) => (found != path).then_some(found),
                Looked::Missing => None,
                Looked::Unread => {
                    unread += 1;
                    None
                }
            }
        };
        for path in std::iter::once(path).chain(found) {
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    (out, unread)
}

/// The full short form of the local path `path` (`GetShortPathNameW`): [`Looked::Missing`]
/// for a path that does not exist, [`Looked::Unread`] when it exists and the call fails; off
/// Windows the path itself.
pub(crate) fn short_form(path: &Path) -> Looked {
    #[cfg(windows)]
    {
        match crate::network_paths::short_path(path) {
            Some(found) => Looked::Form(found),
            None if matches!(path.try_exists(), Ok(false)) => Looked::Missing,
            None => Looked::Unread,
        }
    }
    #[cfg(not(windows))]
    {
        Looked::Form(path.to_path_buf())
    }
}

#[cfg(test)]
#[path = "short_forms_tests.rs"]
mod tests;
