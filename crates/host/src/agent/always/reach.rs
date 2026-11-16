//! What a kept rule reaches, checked on the matter thread by real path: once before the card
//! offers "Always for this matter" (keep time) and again before the host answers a later request
//! for the lawyer (every use). A rule whose reach no longer holds does not apply, and the lawyer
//! is asked (decision record 0063, "Commands").
//!
//! - A command's working folder and every path word (`table::Word::Path`), resolved through
//!   every link and junction, must land inside the matter: a name the agent swapped for a link to
//!   another matter's file after the rule was kept is caught here.
//! - Its program must be found on the agent's search path at the place its allowlist entry names
//!   (`table::At`), by real path: never a file in the matter, never a relative entry.
//! - For Codex, no entry of the working folder may bear the program's name: codex-acp 2.2.2's
//!   `stripShellPrefix` shows a run of `bash <name>` (a script in that folder) as `<name>`.
//!
//! What is checked is the state at that moment; a change between the check and the run is not
//! seen, as for every check of the host's on the matter thread.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::table::At;
use super::{real_inside, real_within};

/// What a kept rule's request reaches, for [`Reach::check`]. Empty for a rule that reaches
/// nothing on disk (a site, a tool).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Reach {
    /// Files strictly inside the matter, in no folder of its own (`always::real_within`): a
    /// `Read` rule's file, every file of an `Edit(*)` request.
    pub(crate) files: Vec<PathBuf>,
    /// A command's working folder and path words, absolute: each the matter folder or inside
    /// it, in no folder of its own (`always::real_inside`).
    pub(crate) places: Vec<PathBuf>,
    /// A command's program.
    pub(crate) program: Option<Program>,
    /// A path that must not exist, not even as a link: Codex's working folder joined with its
    /// program's name.
    pub(crate) absent: Option<PathBuf>,
}

/// A command's program, as the agent's shell would find it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Program {
    /// Its name as typed.
    pub(crate) name: String,
    /// Where its allowlist entry says it may be.
    pub(crate) at: At,
    /// The agent's search path, `PATH` as its launch set it; none when not known, and then the
    /// program is never found.
    pub(crate) search: Option<OsString>,
    /// Clauseo's own folders first on that path, for [`At::Bundled`].
    pub(crate) bundled: Vec<PathBuf>,
}

impl Reach {
    /// Whether it holds nothing to check.
    pub(crate) fn is_empty(&self) -> bool {
        self.files.is_empty()
            && self.places.is_empty()
            && self.program.is_none()
            && self.absent.is_none()
    }

    /// Whether everything it reaches holds now, for the matter `folder`; none when it holds
    /// nothing to check. File system calls: off the actor.
    pub(crate) fn check(&self, folder: &Path, windows: bool) -> Option<bool> {
        if self.is_empty() {
            return None;
        }
        let files = self
            .files
            .iter()
            .all(|file| real_within(file, folder, windows));
        let places = self
            .places
            .iter()
            .all(|place| real_inside(place, folder, windows));
        let program = self
            .program
            .as_ref()
            .is_none_or(|program| program.found(folder, windows));
        let absent = self.absent.as_ref().is_none_or(|path| {
            std::fs::symlink_metadata(path)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        });
        Some(files && places && program && absent)
    }
}

impl Program {
    /// Whether the agent's shell would run this program from a place its entry names: the
    /// first entry of the search path holding a file of its name (on Windows also with `.exe`,
    /// `.com`, `.bat` or `.cmd`) is, by real path, in one of the entry's folders and outside the
    /// matter `folder`. A relative entry before it fails. A cmdlet is part of the shell.
    pub(crate) fn found(&self, folder: &Path, windows: bool) -> bool {
        let allowed: Vec<PathBuf> = match self.at {
            At::Cmdlet => return true,
            At::System { unix, windows: win } => {
                let listed = if windows { win } else { unix };
                listed.iter().map(PathBuf::from).collect()
            }
            At::Bundled => self.bundled.clone(),
        };
        let Some(search) = self.search.as_ref() else {
            return false;
        };
        let Some(hit) = first_on(search, &self.name, windows) else {
            return false;
        };
        let Ok(real) = dunce::canonicalize(&hit) else {
            return false;
        };
        let file_name = real
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase());
        let named = file_name.as_deref().is_some_and(|name| {
            name == self.name.to_lowercase()
                || (windows && name == format!("{}.exe", self.name.to_lowercase()))
        });
        let in_allowed = real.parent().is_some_and(|parent| {
            allowed.iter().any(|allowed| {
                dunce::canonicalize(allowed).is_ok_and(|allowed| allowed.as_path() == parent)
            })
        });
        named && in_allowed && !real_inside(&real, folder, windows) && real.is_file()
    }
}

/// The first file named `name` (on Windows with an extension a shell would add) in the
/// entries of `search`, in order; none when a relative entry comes first or none holds it.
fn first_on(search: &OsString, name: &str, windows: bool) -> Option<PathBuf> {
    let names: Vec<String> = if windows {
        ["", ".exe", ".com", ".bat", ".cmd"]
            .iter()
            .map(|suffix| format!("{name}{suffix}"))
            .collect()
    } else {
        vec![name.to_owned()]
    };
    for entry in std::env::split_paths(search) {
        if entry.as_os_str().is_empty() || !entry.is_absolute() {
            return None;
        }
        for name in &names {
            let candidate = entry.join(name);
            if std::fs::symlink_metadata(&candidate).is_ok() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "reach_tests.rs"]
mod tests;
