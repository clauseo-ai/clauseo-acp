//! Reusing the lawyer's Codex login: the hard link, the guard and the split rule (the brief,
//! "Codex, reused", "The split rule" and "The login source"). The one file under `src` that may
//! name Codex's login file, and it never opens or reads it (`tests/credential_names.rs`).
//!
//! Every operation here on a login file's name, for the security review:
//! - `std::fs::symlink_metadata` (an `lstat`, which opens nothing) to see whether a name is
//!   there and is a plain file;
//! - [`crate::same_file`], which compares file ids (an `lstat` on Unix; on Windows a handle with
//!   access 0, which cannot read a byte; `fileid.rs`);
//! - `crate::write::hard_link` (`std::fs::hard_link`), which opens neither file;
//! - `crate::write::remove_file` (`std::fs::remove_file`), which removes the home's name and
//!   opens nothing.
//!
//! Claude needs nothing here: its reuse is an environment (`CLAUDE_SECURESTORAGE_CONFIG_DIR=""`
//! with Clauseo's `CLAUDE_CONFIG_DIR`, `signin/env.rs`), and "Claude needs no link anywhere".

use std::io;
use std::path::{Path, PathBuf};

use crate::fileid::same_file;
use crate::signin::Found;
use crate::signin::marker::LoginSource;

/// Codex's login file's name, in a `CODEX_HOME` and in `~/.codex`.
const LOGIN_FILE: &str = "auth.json";

/// The lawyer's own Codex folder under their home folder.
const LAWYER_CODEX: &str = ".codex";

/// The login file in a Codex home (Clauseo's, or a matter's state folder in stage C).
fn home_file(codex_home: &Path) -> PathBuf {
    codex_home.join(LOGIN_FILE)
}

/// The lawyer's login file, `<lawyer_home>/.codex/<login file>`.
fn lawyer_file(lawyer_home: &Path) -> PathBuf {
    lawyer_home.join(LAWYER_CODEX).join(LOGIN_FILE)
}

/// What the split check found: [`codex_link_holds`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CodexLink {
    /// The home's login file and the lawyer's are still one file: the launch may go ahead.
    Holds,
    /// They are no longer one file, "for any reason (a rename-save on either side, or the lawyer
    /// signing out and back in, which stat cannot tell apart)", or the home's name is gone or is
    /// a link: `SignedOut { found: CodexLinkSplit }`.
    Split,
    /// The lawyer's own file is gone (they signed out of Codex): "the same steps with
    /// `ReusedSignedOut`".
    Gone,
}

impl CodexLink {
    /// Why reuse stops, or `None` while the link holds.
    pub fn found(self) -> Option<Found> {
        match self {
            CodexLink::Holds => None,
            CodexLink::Split => Some(Found::CodexLinkSplit),
            CodexLink::Gone => Some(Found::ReusedSignedOut),
        }
    }
}

/// The split rule's check, which R20 (before stage C) and R17 (per matter) run at every Codex
/// session start while the source is reused. From the brief: "every Codex session start … stats
/// the home's `auth.json` and `~/.codex/auth.json` and compares file ids. If they are no longer
/// one file, for any reason …, reuse stops (with `~/.codex/auth.json` gone, the same steps with
/// `ReusedSignedOut`): the launch is refused, the state becomes `SignedOut { found:
/// CodexLinkSplit }`, R20's leave order runs …, and UD1 or UD2 shows Clauseo's own sign-in.
/// Clauseo never falls back to either file and never re-links on its own."
///
/// `codex_home` is the session's `CODEX_HOME`; `lawyer_home` the lawyer's home folder (where
/// Codex finds `~/.codex`). Only file ids are compared, never contents, and links are not
/// followed: a symbolic link in either place is never "one file".
pub fn codex_link_holds(codex_home: &Path, lawyer_home: &Path) -> CodexLink {
    let lawyer = lawyer_file(lawyer_home);
    match std::fs::symlink_metadata(&lawyer) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => return CodexLink::Gone,
        Err(_) => return CodexLink::Split,
        Ok(_) => {}
    }
    match same_file(&home_file(codex_home), &lawyer) {
        Ok(true) => CodexLink::Holds,
        Ok(false) | Err(_) => CodexLink::Split,
    }
}

/// The guard. From the brief: "before any Codex `login`, `logout` or `authenticate` (S18's or
/// R20's), stat the home's `auth.json` and `~/.codex/auth.json` and refuse while they are the
/// same file (same file id), whatever the marker says." True means refuse: the home's login is
/// the lawyer's file, and Codex's `logout` "revokes the tokens and deletes the lawyer's login".
/// S18 calls it before each of its own `login` and `logout` runs; R20 before codex-acp's
/// `authenticate`.
pub fn codex_login_shared(codex_home: &Path, lawyer_home: &Path) -> bool {
    same_file(&home_file(codex_home), &lawyer_file(lawyer_home)).unwrap_or(false)
}

/// The leave order's unlink step. From the brief: "Leaving a reused Codex login follows R20's
/// order (no-reuse, stop, unlink, then the new login, then relaunch); S18 supplies the steps and
/// removes the home's link with `remove_file`, which opens nothing." Removes the login file's
/// name in `codex_home`; the lawyer's own name, and so their login, stay. A missing name is
/// fine. Call it once every Codex process using `codex_home` has stopped (R20's "stop"), so
/// none writes the name again; [`crate::Agents::stop_using_login`] calls it for the agent's own
/// home, and R17 for each matter's state folder.
pub fn unlink_reused_login(codex_home: &Path) -> io::Result<()> {
    crate::write::remove_file(&home_file(codex_home))
}

/// Detection's stat: whether the lawyer's login file is there ("a stat (never an open) of
/// `~/.codex/auth.json`: present is a file-mode login, reusable; absent is a keyring or
/// Windows secrets login").
pub(crate) fn lawyer_login_present(lawyer_home: &Path) -> bool {
    std::fs::symlink_metadata(lawyer_file(lawyer_home)).is_ok()
}

/// Whether a Codex home's login is the lawyer's, reused: `login-source` says `reused`, or, per
/// the brief, "for Codex a missing file with a home `auth.json` is `reused`".
pub(crate) fn codex_reused(source: Option<&LoginSource>, codex_home: &Path) -> bool {
    match source {
        Some(LoginSource::Reused { .. }) => true,
        Some(LoginSource::Own) => false,
        None => std::fs::symlink_metadata(home_file(codex_home)).is_ok(),
    }
}

/// Makes the reused login: the home's login file becomes a hard link to the lawyer's, "one file
/// under two names" (`std::fs::hard_link`, which opens nothing), only on the lawyer's "Use it".
/// A home name that is already the lawyer's file is kept. Err for anything else: the lawyer's
/// name is not a plain file (a link is never linked), the home's name is taken, another volume,
/// or the new name does not compare as one file with the lawyer's (it is then removed). The
/// caller then falls back to Clauseo's own sign-in, logged.
pub(crate) fn link_reused_login(codex_home: &Path, lawyer_home: &Path) -> io::Result<()> {
    let lawyer = lawyer_file(lawyer_home);
    let ours = home_file(codex_home);
    if !std::fs::symlink_metadata(&lawyer)?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the lawyer's login is not a plain file",
        ));
    }
    crate::write::create_dir_all(codex_home)?;
    if same_file(&ours, &lawyer).unwrap_or(false) {
        return Ok(());
    }
    crate::write::hard_link(&lawyer, &ours)?;
    if same_file(&ours, &lawyer).unwrap_or(false) {
        Ok(())
    } else {
        crate::write::remove_file(&ours)?;
        Err(io::Error::other("the new link is not the lawyer's file"))
    }
}
