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
//! `network_paths::fold_name`, as `matter_fence::key_as` folds on #1085's branch), and there
//! with and without the data volume's `/System/Volumes/Data` (a firmlink twin, [`keys`]); as
//! they are on Linux.

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
    /// Its other spellings (the launch prep's `folders::spellings`): where it leads, that as on
    /// disk, the firmlink twins.
    pub(crate) also: Vec<PathBuf>,
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
            also: Vec::new(),
        }
    }
}

/// What [`other_denies`] gives: the spellings to deny, and the names of the matters where
/// their folders lead was left out.
#[derive(Debug, Default)]
pub(super) struct OtherDenies {
    /// Each stored folder, and its other spellings but those [`left_open`], each once.
    pub(super) spellings: Vec<PathBuf>,
    /// The names of the matters where it leads was left out, each once.
    pub(super) left_out: Vec<String>,
}

/// The read-deny spellings of `others` on `os` (the module doc): each stored folder, then its
/// other spellings but those [`left_open`] against `open` (the session's own matter, each
/// spelling, and every folder its lists open), with one warn line for each matter one was left
/// out of, naming no path.
pub(super) fn other_denies(os: SandboxOs, others: &[OtherMatter], open: &[&Path]) -> OtherDenies {
    let mut out = OtherDenies::default();
    for other in others {
        let (kept, left): (Vec<&PathBuf>, Vec<&PathBuf>) = other
            .also
            .iter()
            .partition(|also| !left_open(os, also, &other.stored, open));
        if !left.is_empty() {
            tracing::warn!(
                matter = ?other.id,
                "another matter's folder leads to a place Claude's sandbox must leave open; \
                 only its other spellings are denied"
            );
            if !out.left_out.contains(&other.name) {
                out.left_out.push(other.name.clone());
            }
        }
        for spelling in kept.into_iter().chain([&other.stored]) {
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
    let at = keys(os, walked);
    if at.iter().any(|at| at.len() <= 1) {
        return true;
    }
    let under = |place: &Path| holds(&keys(os, place), &at);
    under(stored)
        || open.iter().any(|place| under(place))
        || SHELL_NEEDS.iter().any(|place| under(Path::new(place)))
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

/// [`key`] of `path`, and on macOS, when it starts with the data volume
/// (`/System/Volumes/Data`, folded), also without it: `/System/Volumes/Data/Users/a` is
/// `/Users/a` through a firmlink, so comparing every key of two paths meets either spelling.
pub(crate) fn keys(os: SandboxOs, path: &Path) -> Vec<Vec<String>> {
    let key = key(os, path);
    let mut out = Vec::with_capacity(2);
    if os == SandboxOs::Mac
        && let [rootdir, system, volumes, data, ..] = key.as_slice()
        && rootdir == "/"
        && [system, volumes, data] == ["system", "volumes", "data"]
    {
        let mut stripped = key.clone();
        stripped.drain(1..4);
        out.push(stripped);
    }
    out.push(key);
    out
}

/// Whether a key of `places` starts with a key of `at`: `at` is the place or above it.
pub(crate) fn holds(places: &[Vec<String>], at: &[Vec<String>]) -> bool {
    places
        .iter()
        .any(|place| at.iter().any(|at| place.starts_with(at)))
}

/// `text` without a leading data volume (`/System/Volumes/Data`, any case) on macOS: its
/// firmlinked form; none when it has none.
pub(crate) fn off_data_volume(os: SandboxOs, text: &str) -> Option<String> {
    const DATA: &str = "/system/volumes/data/";
    let head = text.get(..DATA.len())?;
    (os == SandboxOs::Mac && head.eq_ignore_ascii_case(DATA))
        .then(|| text.get(DATA.len() - 1..).unwrap_or("/").to_owned())
}

/// `text` folded as [`key`] folds a name on `os`.
pub(crate) fn fold(os: SandboxOs, text: &str) -> String {
    if os == SandboxOs::Mac {
        crate::network_paths::fold_name(text)
    } else {
        text.to_owned()
    }
}
