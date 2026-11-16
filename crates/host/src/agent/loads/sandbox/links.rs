//! Where another matter's folder leads, in Claude's sandbox's read denies ([`other_denies`]).
//!
//! Claude Code's sandbox resolves no link in a deny, so a matter registered through one is
//! denied as stored and where it leads (the launch prep's walk, `launch_checks::folders`). But
//! where it leads may be the agent's choice: a matter nested in another one is writable from
//! the outer matter's shell on main, so its agent can swap the inner folder for a link to `/`.
//! Denying `/`, a folder above the session's own, or one the shell needs would break every
//! command in every matter with no word of why. So where a folder leads is left out of the
//! denies when it is [`left_open`]: `/`, the session's own matter or a folder above it, one of
//! the folders its lists open or a folder above one, a system folder the shell reads
//! ([`SHELL_NEEDS`]) or one above it, or a folder above the stored folder it came from. The
//! stored spelling (the link) stays denied, and the launch tells the lawyer the other matter's
//! folder now leads somewhere unexpected (`NoticeKind::OtherMatterLinkUnexpected`). Any other
//! place stays denied: a link to `~/Documents` fails closed there, which is accepted.
//!
//! Paths compare by name, folded as Seatbelt matches on macOS (case and Unicode form,
//! `network_paths::fold_name`, as `matter_fence::key_as` folds on #1085's branch), as they are
//! on Linux.

use std::path::{Component, Path, PathBuf};

use clauseo_protocol::MatterId;

use super::SandboxOs;

/// The system folders a sandboxed shell reads: a deny of one, or of a folder above one, breaks
/// every command (`/bin/sh` unreadable). `/lib` and `/lib64` are Linux's.
pub(crate) const SHELL_NEEDS: [&str; 14] = [
    "/bin",
    "/sbin",
    "/usr",
    "/System",
    "/Library",
    "/private",
    "/etc",
    "/var",
    "/tmp",
    "/dev",
    "/opt",
    "/Applications",
    "/lib",
    "/lib64",
];

/// Another matter whose folder Claude's sandbox denies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OtherMatter {
    /// The matter, for the logs; none in tests.
    pub(crate) id: Option<MatterId>,
    /// Its name as the lawyer sees it, for the launch's notice; never a path.
    pub(crate) name: String,
    /// Its folder as stored.
    pub(crate) stored: PathBuf,
    /// Where that leads (the network-safe walk), when it differs from `stored`.
    pub(crate) walked: Option<PathBuf>,
}

impl OtherMatter {
    /// A matter at `stored` with no other spelling, named by its folder's last name.
    pub(crate) fn at(stored: impl Into<PathBuf>) -> Self {
        let stored = stored.into();
        let name = stored
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        Self {
            id: None,
            name,
            stored,
            walked: None,
        }
    }
}

/// What [`other_denies`] gives: the spellings to deny, and the names of the matters where
/// their folders lead was left out.
#[derive(Debug, Default)]
pub(super) struct OtherDenies {
    /// Each stored folder, and where it leads unless that is [`left_open`], each once.
    pub(super) spellings: Vec<PathBuf>,
    /// The names of the matters where it leads was left out, each once.
    pub(super) left_out: Vec<String>,
}

/// The read-deny spellings of `others` on `os` (the module doc): each stored folder, then where
/// it leads unless [`left_open`] against `open` (the session's own matter, each spelling, and
/// every folder its lists open), with one warn line for each one left out, naming no path.
pub(super) fn other_denies(os: SandboxOs, others: &[OtherMatter], open: &[&Path]) -> OtherDenies {
    let mut out = OtherDenies::default();
    for other in others {
        let walked = other.walked.as_ref().filter(|walked| {
            let left = left_open(os, walked, &other.stored, open);
            if left {
                tracing::warn!(
                    matter = ?other.id,
                    "another matter's folder leads to a place Claude's sandbox must leave open; \
                     only its stored folder is denied"
                );
                if !out.left_out.contains(&other.name) {
                    out.left_out.push(other.name.clone());
                }
            }
            !left
        });
        for spelling in walked.into_iter().chain([&other.stored]) {
            if !out.spellings.contains(spelling) {
                out.spellings.push(spelling.clone());
            }
        }
    }
    out
}

/// Whether `walked`, where another matter's `stored` folder leads, is a place the sandbox must
/// leave open (the module doc): `/`, a folder above (or the same as) `stored`, any of `open`
/// or any of [`SHELL_NEEDS`]. Compared by [`key`].
pub(crate) fn left_open(os: SandboxOs, walked: &Path, stored: &Path, open: &[&Path]) -> bool {
    let at = key(os, walked);
    if at.len() <= 1 {
        return true;
    }
    let holds = |place: &Path| key(os, place).starts_with(&at);
    holds(stored)
        || open.iter().any(|place| holds(place))
        || SHELL_NEEDS.iter().any(|place| holds(Path::new(place)))
}

/// `path`'s names for a prefix comparison on `os`: each folded on macOS as Seatbelt matches it
/// (`network_paths::fold_name`: case and Unicode form), as it is on Linux; `.` dropped.
pub(crate) fn key(os: SandboxOs, path: &Path) -> Vec<String> {
    path.components()
        .filter(|part| *part != Component::CurDir)
        .map(|part| {
            let text = part.as_os_str().to_string_lossy();
            match part {
                Component::Normal(_) if os == SandboxOs::Mac => {
                    crate::network_paths::fold_name(&text)
                }
                _ => text.into_owned(),
            }
        })
        .collect()
}

/// `text` folded as [`key`] folds a name on `os`.
pub(crate) fn fold(os: SandboxOs, text: &str) -> String {
    if os == SandboxOs::Mac {
        crate::network_paths::fold_name(text)
    } else {
        text.to_owned()
    }
}
