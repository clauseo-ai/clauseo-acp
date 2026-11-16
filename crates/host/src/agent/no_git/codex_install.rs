//! A Windows Codex install's own folders, which Codex looks in for `git` before `PATH` and which
//! Clauseo checks at each launch and before every prompt and session call. In the pinned
//! `@openai/codex` 0.162.1 codex.exe is
//! `node_modules/@openai/codex-win32-<arch>/vendor/<triple>/bin/codex.exe` and `codex-path` sits
//! beside `bin`. A Codex launch on Windows is refused unless the install's Codex packages, their
//! `vendor` folders, every `vendor/<triple>` folder and every folder in it can be read and hold
//! no link, and those last two hold no git program ([`beside_codex`]). The agents' start-up
//! removes a git program at exactly those two levels (`clauseo-agents`' `stray_git.rs`), so a
//! restart clears every refusal for one; a link, a folder that can't be read or no
//! `vendor/<triple>` at all stays a refusal a restart does not clear.

use std::path::{Component, Path, PathBuf};

use clauseo_process::WINDOWS_GIT_PROGRAMS;
use clauseo_settings::AgentLaunch;

/// Why a Codex install's folders are not vouched for, as a word for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Beside {
    /// A git program sits in one.
    Git,
    /// No `vendor/<triple>` folder was found.
    NoVendor,
    /// A folder could not be read.
    Unreadable,
    /// An entry is a link or a junction.
    Link,
}

impl Beside {
    /// The word a log line shows.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Git => "git",
            Self::NoVendor => "no_vendor",
            Self::Unreadable => "unreadable",
            Self::Link => "link",
        }
    }
}

/// The install folder of `agent`'s entry: the part of its command or of its first argument
/// that names a path, before a `node_modules` part (the adapter's script,
/// `<install>/node_modules/<root>/<bin>`). None, with a warning, when neither does: the launch
/// goes on without this check.
pub(crate) fn install_of(agent: &AgentLaunch) -> Option<PathBuf> {
    let found = std::iter::once(&agent.command)
        .chain(&agent.args)
        .find_map(|word| before_node_modules(Path::new(word)));
    if found.is_none() {
        tracing::warn!("no Codex install folder in the agent's entry: its folders go unchecked");
    }
    found
}

/// `path` up to (not including) its first `node_modules` part, when it has one after a root.
fn before_node_modules(path: &Path) -> Option<PathBuf> {
    let mut install = PathBuf::new();
    for part in path.components() {
        if part == Component::Normal("node_modules".as_ref()) {
            return install.is_absolute().then_some(install);
        }
        install.push(part);
    }
    None
}

/// `Ok` when `node_modules`, `@openai` and each package's `vendor` are no link, every
/// `vendor/<triple>` folder of `install`'s Codex packages (`node_modules/@openai/*`), and every
/// folder in it, was read, holds no link and no git program ([`WINDOWS_GIT_PROGRAMS`]), and at
/// least one such folder exists; else why not.
pub(crate) fn beside_codex(install: &Path) -> Result<(), Beside> {
    let modules = install.join("node_modules");
    let packages = modules.join("@openai");
    not_a_link(&modules)?;
    not_a_link(&packages)?;
    let mut triples = 0_usize;
    for package in folders(&packages, false)? {
        let vendor = package.join("vendor");
        if !vendor.exists() && std::fs::symlink_metadata(&vendor).is_err() {
            continue;
        }
        not_a_link(&vendor)?;
        for triple in folders(&vendor, false)? {
            triples += 1;
            for inner in folders(&triple, true)? {
                folders(&inner, true)?;
            }
        }
    }
    if triples == 0 {
        return Err(Beside::NoVendor);
    }
    Ok(())
}

/// `Err(Link)` when `path` itself is a link or a junction; a path that isn't there passes (the
/// listing after it refuses a missing folder).
fn not_a_link(path: &Path) -> Result<(), Beside> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(Beside::Link),
        _ => Ok(()),
    }
}

/// The folders directly in `dir`, once every entry of it was read and none is a link or a
/// junction (`is_symlink` reports both on Windows), nor, with `git`, named as a git program.
fn folders(dir: &Path, git: bool) -> Result<Vec<PathBuf>, Beside> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|_unread| Beside::Unreadable)? {
        let entry = entry.map_err(|_unread| Beside::Unreadable)?;
        let kind = entry.file_type().map_err(|_unread| Beside::Unreadable)?;
        if kind.is_symlink() {
            return Err(Beside::Link);
        }
        let name = entry.file_name();
        if git
            && WINDOWS_GIT_PROGRAMS
                .iter()
                .any(|program| name.eq_ignore_ascii_case(program))
        {
            return Err(Beside::Git);
        }
        if kind.is_dir() {
            found.push(entry.path());
        }
    }
    Ok(found)
}
