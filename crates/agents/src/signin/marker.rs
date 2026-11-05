//! Clauseo's own two sign-in markers, in the agent's data folder `<data_dir>/agents/<id>/`,
//! never inside a CLI's home (the brief, "The login source"):
//!
//! - `login-source`: "`reused` with the accepted email, or `own`; no secret", "written on \"Use
//!   it\" or when Clauseo's sign-in ends". The text is one line, `own`, or two, `reused` and
//!   then the accepted email (the second line is left out when the status reported none). For
//!   Codex "a missing file with a home `auth.json` is `reused`": that rule is `signin/reuse.rs`'s,
//!   which may name the file.
//! - `no-reuse`: written by "Sign in with a different account" and "Stop using it here", and
//!   "detection skips that agent while it exists". Its text is the one line `no-reuse`; only
//!   whether it exists counts.
//!
//! Both are read and written through `crate::write`, which refuses a link where a marker goes,
//! and are written atomically. They live here and not in `reuse.rs`, which may not read a file.

use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::AgentId;

/// The most a marker may hold: two short lines.
const LIMIT: u64 = 4096;

/// The first line of `login-source` for each source.
const OWN: &str = "own";
const REUSED: &str = "reused";

/// `no-reuse`'s text.
const NO_REUSE_TEXT: &str = "no-reuse\n";

/// Where the agent's login came from, as `login-source` records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoginSource {
    /// Clauseo's own sign-in.
    Own,
    /// The lawyer's own login, reused after "Use it", with the email they accepted.
    Reused {
        /// The accepted email, from the agent's own status output.
        email: Option<String>,
    },
}

impl LoginSource {
    /// The marker's text.
    fn text(&self) -> String {
        match self {
            LoginSource::Own => format!("{OWN}\n"),
            LoginSource::Reused { email: None } => format!("{REUSED}\n"),
            LoginSource::Reused { email: Some(email) } => format!("{REUSED}\n{email}\n"),
        }
    }

    /// The source `text` records. Anything but `own` reads as reused: an unreadable or garbled
    /// marker must never let a logout run on what may be the lawyer's login.
    fn parse(text: &str) -> LoginSource {
        let mut lines = text.lines().map(str::trim);
        match lines.next() {
            Some(OWN) => LoginSource::Own,
            Some(REUSED) => LoginSource::Reused {
                email: lines
                    .next()
                    .filter(|email| !email.is_empty() && !email.contains(char::is_control))
                    .map(str::to_owned),
            },
            _ => LoginSource::Reused { email: None },
        }
    }
}

/// `<data_dir>/agents/<id>`: the agent's data folder.
pub(crate) fn agent_folder(data_dir: &Path, agent: &AgentId) -> PathBuf {
    data_dir.join("agents").join(agent.as_str())
}

fn source_path(data_dir: &Path, agent: &AgentId) -> PathBuf {
    agent_folder(data_dir, agent).join("login-source")
}

fn no_reuse_path(data_dir: &Path, agent: &AgentId) -> PathBuf {
    agent_folder(data_dir, agent).join("no-reuse")
}

/// The recorded source, `None` when there is no marker. A marker that cannot be read (a link
/// stands there, it is too long, it is not text) reads as reused, the safe side.
pub(crate) fn read_source(data_dir: &Path, agent: &AgentId) -> Option<LoginSource> {
    match crate::write::read_small(&source_path(data_dir, agent), LIMIT) {
        Ok(None) => None,
        Ok(Some(text)) => Some(LoginSource::parse(&text)),
        Err(_) => Some(LoginSource::Reused { email: None }),
    }
}

/// Records `source`.
pub(crate) fn write_source(
    data_dir: &Path,
    agent: &AgentId,
    source: &LoginSource,
) -> io::Result<()> {
    crate::write::create_private_dirs_under(data_dir, &agent_folder(data_dir, agent))?;
    crate::write::write_atomic(&source_path(data_dir, agent), source.text().as_bytes())
}

/// Clears the reuse record: `login-source` goes. A missing one is fine.
pub(crate) fn clear_source(data_dir: &Path, agent: &AgentId) -> io::Result<()> {
    crate::write::remove_file(&source_path(data_dir, agent))
}

/// Whether `no-reuse` is there (a link counts, and is never followed).
pub(crate) fn no_reuse(data_dir: &Path, agent: &AgentId) -> bool {
    std::fs::symlink_metadata(no_reuse_path(data_dir, agent)).is_ok()
}

/// Writes `no-reuse`.
pub(crate) fn write_no_reuse(data_dir: &Path, agent: &AgentId) -> io::Result<()> {
    crate::write::create_private_dirs_under(data_dir, &agent_folder(data_dir, agent))?;
    crate::write::write_atomic(&no_reuse_path(data_dir, agent), NO_REUSE_TEXT.as_bytes())
}

/// Deletes `no-reuse` (`find_login`). A missing one is fine.
pub(crate) fn remove_no_reuse(data_dir: &Path, agent: &AgentId) -> io::Result<()> {
    crate::write::remove_file(&no_reuse_path(data_dir, agent))
}

#[cfg(test)]
mod tests;
