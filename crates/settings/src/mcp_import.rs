//! Finds the MCP servers a lawyer already set up in Claude Desktop, Claude Code and Codex, and
//! turns them into import candidates.
//!
//! # Owns
//!
//! The three readers, one per app (Claude Desktop, Claude Code and Codex): where each
//! keeps its config on each OS, what an entry means and which entries are usable. Also the
//! `${VAR}` expansion Claude Code's text takes, the command search path, and the name rules
//! (those of `leaf/mcp.rs`, plus `browser` and `case-law` reserved).
//!
//! # Interface
//!
//! [`find_candidates`] takes an [`Env`]: the OS, the home folder, the environment variables and an
//! [`ImportFs`]. [`RealFs`] reads the disk; tests pass a fake. Nothing here writes a file or
//! starts a process.
//!
//! # Invariants
//!
//! - The `Debug` output of [`ImportCandidate`] and [`CandidateKind`] names the env and header
//!   entries and never prints their values. A URL prints without its query string.
//! - Output is sorted by name. Every name passes the MCP name rules, is unique among the
//!   candidates and is not one of the names in `existing_names` unless it is marked
//!   [`Problem::AlreadyAdded`].
//! - A file that cannot be read or parsed gives no candidates; nothing panics on bad input.

mod claude_code;
mod claude_desktop;
mod codex;
mod command;
mod expand;
mod json_entry;
mod name;
mod path;
#[cfg(test)]
mod tests;

use std::fmt;
use std::path::{Path, PathBuf};

/// The operating system whose rules the import follows. Passed in, so the Windows rules are
/// tested on any machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// macOS.
    Mac,
    /// Windows.
    Windows,
}

impl Os {
    /// The OS this build runs on, or `None` where Clauseo is not built.
    pub fn current() -> Option<Os> {
        if cfg!(target_os = "macos") {
            Some(Os::Mac)
        } else if cfg!(windows) {
            Some(Os::Windows)
        } else {
            None
        }
    }
}

/// Where the import looks: the OS, the home folder, the environment variables and the filesystem.
pub struct Env<'a> {
    /// The OS whose paths and command rules apply.
    pub os: Os,
    /// The lawyer's home folder.
    pub home: &'a Path,
    /// The value of an environment variable, or `None` when it is not set.
    pub vars: &'a dyn Fn(&str) -> Option<String>,
    /// The reads the import makes.
    pub fs: &'a dyn ImportFs,
}

/// The reads the import makes. [`RealFs`] is the disk; tests use a fake.
pub trait ImportFs {
    /// The text of the file at `p`, or `None` when it cannot be read.
    fn read_to_string(&self, p: &Path) -> Option<String>;
    /// Whether `p` is a file (a symlink to one counts).
    fn is_file(&self, p: &Path) -> bool;
    /// The entries of the folder `p`, as full paths; empty when it cannot be read.
    fn list_dir(&self, p: &Path) -> Vec<PathBuf>;
}

/// [`ImportFs`] over `std::fs`.
pub struct RealFs;

impl ImportFs for RealFs {
    fn read_to_string(&self, p: &Path) -> Option<String> {
        std::fs::read_to_string(p).ok()
    }

    fn is_file(&self, p: &Path) -> bool {
        p.is_file()
    }

    fn list_dir(&self, p: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(p)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// The process's own environment variables, for [`Env::vars`].
pub fn process_vars(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// One server an app has set up, ready to import.
#[derive(Clone, PartialEq, Eq)]
pub struct ImportCandidate {
    /// The name it gets in Clauseo: valid under the MCP name rules and unique.
    pub name: String,
    /// The apps and folders that set it up with this same config.
    pub sources: Vec<Source>,
    /// How to start it or reach it.
    pub kind: CandidateKind,
    /// What the lawyer should know before importing it.
    pub problems: Vec<Problem>,
}

impl fmt::Debug for ImportCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportCandidate")
            .field("name", &self.name)
            .field("sources", &self.sources)
            .field("kind", &self.kind)
            .field("problems", &self.problems)
            .finish()
    }
}

/// Where a candidate was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Claude Desktop's config file.
    ClaudeDesktop,
    /// Claude Code's user-wide `mcpServers` in `.claude.json`.
    ClaudeCodeUser,
    /// Claude Code's servers for one project folder: its `projects` entry or its `.mcp.json`.
    ClaudeCodeProject {
        /// The project folder.
        folder: PathBuf,
    },
    /// Codex's `[mcp_servers.<name>]` table.
    Codex,
}

/// How a candidate runs or is reached. Env and header values are secrets: [`fmt::Debug`] names
/// them and never prints them.
#[derive(Clone, PartialEq, Eq)]
pub enum CandidateKind {
    /// A local program started with `command`.
    Local {
        /// The program's path: resolved when it was found, else the command as given.
        command: PathBuf,
        /// Its arguments.
        args: Vec<String>,
        /// Its environment variables, sorted by name.
        env: Vec<(String, String)>,
    },
    /// A server reached over the network.
    Remote {
        /// The server's address.
        url: String,
        /// The headers sent with each request, sorted by name.
        headers: Vec<(String, String)>,
    },
}

impl fmt::Debug for CandidateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CandidateKind::Local { command, args, env } => f
                .debug_struct("Local")
                .field("command", command)
                .field("args", args)
                .field("env", &names(env))
                .finish(),
            CandidateKind::Remote { url, headers } => f
                .debug_struct("Remote")
                .field("url", &without_query(url))
                .field("headers", &names(headers))
                .finish(),
        }
    }
}

/// The names of a list of pairs, never their values.
fn names(pairs: &[(String, String)]) -> Vec<&str> {
    pairs.iter().map(|(name, _)| name.as_str()).collect()
}

/// The URL up to its query string, which can hold a key.
fn without_query(url: &str) -> String {
    match url.find('?') {
        Some(at) => format!("{}?<query omitted>", &url[..at]),
        None => url.to_owned(),
    }
}

/// Something the lawyer should know about a candidate before importing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The command was not found on the search path; the candidate keeps it as given.
    CommandNotFound {
        /// The command as the app's config wrote it.
        original: String,
    },
    /// The app's transport is one Clauseo does not run (for example `sse`).
    UnsupportedTransport {
        /// The transport's name as the config wrote it.
        kind: String,
    },
    /// A `${VAR}` in the config names a variable that is not set; the text is kept.
    UnresolvedVariable {
        /// The variable's name.
        name: String,
    },
    /// The name was changed to fit Clauseo's rules or to stay unique.
    NameChanged {
        /// The name the app gave it.
        from: String,
    },
    /// Clauseo already has a server with this name.
    AlreadyAdded,
    /// Part of the entry could not be read and was left out.
    Unreadable,
}

/// The entry of one app's config, before names are settled.
pub(crate) struct Entry {
    /// The name the app gave it.
    pub(crate) raw_name: String,
    /// The app or folder it was read from.
    pub(crate) source: Source,
    /// Its kind, with variables expanded and the command resolved.
    pub(crate) kind: CandidateKind,
    /// The problems found while reading it, except the name problems.
    pub(crate) problems: Vec<Problem>,
}

/// Finds the servers the lawyer set up in Claude Desktop, Claude Code and Codex.
///
/// `existing_names` are the names Clauseo already has; a candidate with one of them is marked
/// [`Problem::AlreadyAdded`]. Results are sorted by name.
pub fn find_candidates(env: &Env<'_>, existing_names: &[String]) -> Vec<ImportCandidate> {
    let mut entries = Vec::new();
    claude_desktop::read(env, &mut entries);
    claude_code::read(env, &mut entries);
    codex::read(env, &mut entries);
    name::assemble(entries, existing_names)
}
