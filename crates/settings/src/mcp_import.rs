//! Finds the MCP servers a lawyer already set up in Claude Desktop, Claude Code and Codex, and
//! turns them into import candidates.
//!
//! # Owns
//!
//! The three readers, one per app (Claude Desktop, Claude Code and Codex): where each keeps its
//! config on each OS, what an entry means and which entries are usable. Also the `${VAR}`
//! expansion of the lawyer's own Claude Code and Codex config, the command search path, the name
//! rules (those of `leaf/mcp.rs`, plus `browser` and `case-law` reserved) and the approval rules
//! for a project's `.mcp.json`, which a cloned repository can hold.
//!
//! # Interface
//!
//! [`find_candidates`] takes an [`Env`]: the OS, the home folder, the environment variables and an
//! [`ImportFs`]. [`RealEnv`] builds one over the real process. [`resolve_command`] is the search
//! path on its own. Nothing here writes a file or starts a process.
//!
//! Env and header values, `args` and URLs that come from a variable are [`ImportSecret`]s. The
//! import hands them only to the keychain (A3), never to the settings file.
//!
//! # Invariants
//!
//! - `Debug` of [`ImportCandidate`] and [`CandidateKind`] prints names only: env and header names,
//!   the argument count, a URL's scheme and host, a command's file name. It never prints a value
//!   ([`ImportSecret`] prints `<redacted>`), a folder name, a URL's user name, path or query, or
//!   an argument. [`Source`] prints no project folder.
//! - A project's `.mcp.json` is repository text. Its servers are not expanded, and a server is
//!   imported only when Claude Code has approved it, or is marked [`Problem::NotApproved`]. A
//!   server in `disabledMcpjsonServers` is left out.
//! - Only the lawyer's own config is expanded: `~/.claude.json`'s user scope and Codex's
//!   `config.toml`. `%VAR%` (Windows) is never expanded; `${VAR:-${OTHER}}` closes at the first
//!   `}`.
//! - A program is never searched for in a relative or empty folder, nor on a network share (a
//!   path that starts with two separators). See [`resolve_command`].
//! - Each file is read only up to its cap ([`MAX_CLAUDE_JSON_BYTES`], [`MAX_CONFIG_BYTES`]); a
//!   bigger one gives no candidates, as does one that cannot be read or parsed. A parse error
//!   never echoes the file's text.
//! - Output is sorted by name. Every name passes the MCP name rules, is unique among the
//!   candidates and is not one of the names in `existing_names` unless it is marked
//!   [`Problem::AlreadyAdded`].

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

pub use command::resolve_command;

use std::fmt;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

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

/// The largest `~/.claude.json` the import reads (64 MiB): it keeps history, so it can be large.
pub const MAX_CLAUDE_JSON_BYTES: u64 = 64 * 1024 * 1024;

/// The largest other config file the import reads (16 MiB): a project's `.mcp.json`, Codex's
/// `config.toml` and Claude Desktop's config.
pub const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;

/// The reads the import makes. [`RealFs`] is the disk; tests use a fake.
pub trait ImportFs {
    /// The text of the file at `p`, read up to `max` bytes. `None` when it cannot be read, is not
    /// text, or is longer than `max`; a longer file is never read whole.
    fn read_to_string(&self, p: &Path, max: u64) -> Option<String>;
    /// Whether `p` is a file (a symlink to one counts).
    fn is_file(&self, p: &Path) -> bool;
    /// The entries of the folder `p`, as full paths; empty when it cannot be read.
    fn list_dir(&self, p: &Path) -> Vec<PathBuf>;
}

/// [`ImportFs`] over `std::fs`.
pub struct RealFs;

impl ImportFs for RealFs {
    /// Reads at most `max + 1` bytes: one more than the cap, so a longer file is seen and refused
    /// without being read whole (the `take` pattern of `chromium_policy.rs`).
    fn read_to_string(&self, p: &Path, max: u64) -> Option<String> {
        use std::io::Read;
        let file = std::fs::File::open(p).ok()?;
        let mut text = String::new();
        file.take(max.saturating_add(1))
            .read_to_string(&mut text)
            .ok()?;
        (u64::try_from(text.len()).ok()? <= max).then_some(text)
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

/// The real process as an [`Env`]: the OS this build runs on, the home folder, the process's
/// environment variables and the disk.
pub struct RealEnv {
    os: Os,
    home: PathBuf,
    fs: RealFs,
}

impl RealEnv {
    /// The real process's environment, or `None` on an OS Clauseo is not built for or when there
    /// is no home folder.
    pub fn from_process() -> Option<RealEnv> {
        let os = Os::current()?;
        let home = std::env::home_dir()?;
        Some(RealEnv {
            os,
            home,
            fs: RealFs,
        })
    }

    /// This process as an [`Env`], for [`find_candidates`] or [`resolve_command`].
    pub fn env(&self) -> Env<'_> {
        Env {
            os: self.os,
            home: &self.home,
            vars: &process_vars,
            fs: &self.fs,
        }
    }
}

/// A value the import took from the environment or from a `${VAR}` in config. It is zeroed when
/// dropped, prints as `<redacted>`, and has no `Display`. The import hands it only to the
/// keychain (A3), never to the settings file.
#[derive(Clone, PartialEq, Eq)]
pub struct ImportSecret(Zeroizing<String>);

impl ImportSecret {
    /// Wraps `value`.
    pub(crate) fn new(value: String) -> Self {
        ImportSecret(Zeroizing::new(value))
    }

    /// The value. Hand it to the keychain only (A3); never print it or write it to settings.
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for ImportSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
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
#[derive(Clone, PartialEq, Eq)]
pub enum Source {
    /// Claude Desktop's config file.
    ClaudeDesktop,
    /// Claude Code's user-wide `mcpServers` in `.claude.json`.
    ClaudeCodeUser,
    /// Claude Code's servers for one project folder: its `projects` entry or its `.mcp.json`.
    ClaudeCodeProject {
        /// The project folder. Never printed by `Debug`: a folder name can be a matter's name.
        folder: PathBuf,
    },
    /// Codex's `[mcp_servers.<name>]` table.
    Codex,
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::ClaudeDesktop => f.write_str("ClaudeDesktop"),
            Source::ClaudeCodeUser => f.write_str("ClaudeCodeUser"),
            Source::ClaudeCodeProject { .. } => {
                f.debug_struct("ClaudeCodeProject").finish_non_exhaustive()
            }
            Source::Codex => f.write_str("Codex"),
        }
    }
}

/// How a candidate runs or is reached. Env and header values, args and the URL can hold secrets:
/// [`fmt::Debug`] prints names, a count and the scheme and host only.
#[derive(Clone, PartialEq, Eq)]
pub enum CandidateKind {
    /// A local program started with `command`.
    Local {
        /// The program's path: resolved when it was found, else the command as given.
        command: PathBuf,
        /// Its arguments.
        args: Vec<ImportSecret>,
        /// Its environment variables, sorted by name.
        env: Vec<(String, ImportSecret)>,
    },
    /// A server reached over the network.
    Remote {
        /// The server's address.
        url: ImportSecret,
        /// The headers sent with each request, sorted by name.
        headers: Vec<(String, ImportSecret)>,
    },
}

impl fmt::Debug for CandidateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CandidateKind::Local { command, args, env } => f
                .debug_struct("Local")
                .field("command", &file_name_only(command))
                .field("args", &format_args!("{} not shown", args.len()))
                .field("env", &names(env))
                .finish(),
            CandidateKind::Remote { url, headers } => f
                .debug_struct("Remote")
                .field("url", &scheme_and_host(url.expose()))
                .field("headers", &names(headers))
                .finish(),
        }
    }
}

/// The names of a list of pairs, never their values.
fn names(pairs: &[(String, ImportSecret)]) -> Vec<&str> {
    pairs.iter().map(|(name, _)| name.as_str()).collect()
}

/// The last part of a path, splitting on both separators: the file name, no folder.
fn file_name_only(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.rsplit(['/', '\\']).next().unwrap_or("").to_owned()
}

/// `scheme://host[:port]` of a URL: no user name or password, path, query or fragment.
fn scheme_and_host(url: &str) -> String {
    let Some(at) = url.find("://") else {
        return "<not a URL>".to_owned();
    };
    let scheme = &url[..at];
    let rest = &url[at + 3..];
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    format!("{scheme}://{host}")
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
    /// A server in a project's `.mcp.json` that Claude Code has not approved for this project.
    /// The candidate is offered, but the lawyer should approve it in Claude Code first.
    NotApproved,
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

/// The text of the config file at `path`, when it is a file no bigger than `max` bytes.
pub(crate) fn read_config(env: &Env<'_>, path: &str, max: u64) -> Option<String> {
    let p = Path::new(path);
    if !env.fs.is_file(p) {
        return None;
    }
    env.fs.read_to_string(p, max)
}
