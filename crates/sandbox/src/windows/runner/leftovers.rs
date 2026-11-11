//! What the pool user could leave to start at its next logon, or to run in a later runner,
//! removed (M1): its per-user COM classes, the values under its
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` and `RunOnce`, and every entry of its
//! Startup folder. Run as the pool user by the inner runner right after its first-act sweep,
//! before it reads the launch ([`Kind::LOCAL`], [`remove_all`]), so none of them is left when any
//! code of the launch's runs; nothing else removes them. No kind is reached over COM.
//!
//! The per-user classes (`HKCU\Software\Classes\{CLSID,Interface,AppID,TypeLib}` and their
//! `Wow6432Node` twins) go first: a server registered there for a class a runner makes would run
//! for that runner with the lawyer's logon SID, and a sandbox account has no per-user class of its
//! own to keep.
//!
//! The pool user's scheduled tasks and BITS jobs aren't removed here: the `SYSTEM` task
//! `\Clauseo\SandboxSweep`, which the outer requests at every session's end, deletes the tasks
//! and cancels the jobs, and `SandboxStop` does too. A pool user's BITS job can't run anyway
//! (BITS refuses it a file, 0x800704dd).
//!
//! Each kind is listed, then each item deleted; one kind failing (its list, or an item) is logged
//! and the rest still run. The log carries counts and OS codes only, never a name or a content.
//! The calls are behind [`Leftovers`]; Windows' are `os.rs`.

use std::fmt;
use std::io;

#[cfg(windows)]
pub(super) mod os;

/// A kind of leftover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The pool user's per-user COM class keys, deleted whole.
    Classes,
    /// The values of `HKCU\...\Run` and `RunOnce`.
    RunKeys,
    /// The entries of the pool user's Startup folder (`FOLDERID_Startup`).
    Startup,
}

impl Kind {
    /// Every kind, in the order the inner's first act removes them: the classes first. None is
    /// reached over COM.
    pub(crate) const LOCAL: [Kind; 3] = [Kind::Classes, Kind::RunKeys, Kind::Startup];
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Kind::Classes => "per-user COM class keys",
            Kind::RunKeys => "Run and RunOnce values",
            Kind::Startup => "Startup folder entries",
        })
    }
}

/// The calls a removal makes.
pub(crate) trait Leftovers {
    /// One item found; what it holds is never logged.
    type Item;
    /// Every item of `kind`; none when its place doesn't exist.
    fn list(&mut self, kind: Kind) -> io::Result<Vec<Self::Item>>;
    /// Deletes `item`.
    fn delete(&mut self, kind: Kind, item: &Self::Item) -> io::Result<()>;
}

/// Removes the items of each of `kinds`, in order, logging each kind's count, at `who` (the
/// inner's first act). Whether every kind was listed and every item deleted.
pub(crate) fn remove_all<L: Leftovers>(
    calls: &mut L,
    kinds: &[Kind],
    who: &str,
    log: &mut dyn FnMut(&str),
) -> bool {
    let mut clean = true;
    for &kind in kinds {
        clean &= remove_kind(calls, kind, who, log);
    }
    clean
}

/// Removes one kind's items; whether it was listed and every item deleted.
fn remove_kind<L: Leftovers>(
    calls: &mut L,
    kind: Kind,
    who: &str,
    log: &mut dyn FnMut(&str),
) -> bool {
    let items = match calls.list(kind) {
        Ok(items) => items,
        Err(error) => {
            log(&format!(
                "clauseo-sandbox-run: cleanup ({who}): the {kind} can't be listed ({})",
                code(&error)
            ));
            return false;
        }
    };
    let mut removed = 0usize;
    let mut failed: Option<(usize, String)> = None;
    for item in &items {
        match calls.delete(kind, item) {
            Ok(()) => removed += 1,
            Err(error) => {
                let (count, _) = failed.get_or_insert((0, code(&error)));
                *count += 1;
            }
        }
    }
    match failed {
        None if removed == 0 => true,
        None => {
            log(&format!(
                "clauseo-sandbox-run: cleanup ({who}): removed {removed} {kind}"
            ));
            true
        }
        Some((count, first)) => {
            log(&format!(
                "clauseo-sandbox-run: cleanup ({who}): removed {removed} {kind}, {count} \
                 couldn't be ({first})"
            ));
            false
        }
    }
}

/// The names of `path` below `base`, both full local paths, compared without case: `None`
/// when `path` isn't below `base` or a name is empty, `.` or `..`.
pub(crate) fn below<'a>(path: &'a str, base: &str) -> Option<Vec<&'a str>> {
    let base = base.trim_end_matches('\\');
    let head = path.get(..base.len())?;
    let rest = path.get(base.len()..)?.strip_prefix('\\')?;
    if base.is_empty() || !head.eq_ignore_ascii_case(base) {
        return None;
    }
    let parts: Vec<&str> = rest.split('\\').collect();
    let bad = |part: &&str| part.is_empty() || *part == "." || *part == ".." || part.contains('/');
    (!parts.iter().any(bad)).then_some(parts)
}

/// An error's OS code, or its kind.
fn code(error: &io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => code.to_string(),
        None => error.kind().to_string(),
    }
}

#[cfg(test)]
mod tests;
