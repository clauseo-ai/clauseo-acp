//! The `git` that does nothing on every agent's `PATH` (`HostConfig::with_no_git`, decision
//! record 0030). The desktop copies Clauseo's own `clauseo-mcp` to `<data>/tools/no-git/git`
//! (`git.exe` on Windows), where, run by that name, it prints nothing and fails
//! (`clauseo_mcp::no_git`). For every launch of a Claude or Codex agent:
//!
//! - the launch prep checks the program ([`check_all`]): a regular file, not a link, holding
//!   the bytes whose SHA-256 the desktop copied, alone in its folder, and for a Codex launch on
//!   Windows no `git` program in its install's own folders ([`codex_install`]). Missing (a virus
//!   scanner's quarantine), a link, changed, crowded, never copied, or in a build for the lawyer
//!   never named ([`required`]) refuses the launch with `agent.tool_missing` ([`refusal`]); no
//!   agent starts without it. The same check runs again before every prompt and session call
//!   (`agent::codex::prepare::recheck`), since the program can go mid-session;
//! - its folder goes first on the session's `PATH` (`launch_checks::paths::PathPrep::resolve`),
//!   so the agent's process, Claude's `CLAUDE_ENV_FILE` line and Codex's macOS shell `PATH`
//!   start with it (`agent/shell_path.rs`), and a Codex session's shells get that whole `PATH`
//!   as `shell_environment_policy.set.PATH` ([`codex_shell_path`]);
//! - the launch sets [`ATTR_SOURCE_VAR`] for both agents and, for Codex alone,
//!   [`CEILING_VAR`] ([`launch_env`]).
//!
//! The host's own process never gets the folder: only the agent's launch environment does, and
//! Clauseo's history runs no `git` program (`clauseo-matters` uses gix in-process). Logs hold a
//! word, never a path.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentKind, ErrorCode};
use clauseo_settings::AgentLaunch;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::config::NoGit;
use crate::error::Error;

/// The variable naming the tree git reads attributes from, set for both agents to the empty
/// tree ([`EMPTY_TREE`]).
pub(crate) const ATTR_SOURCE_VAR: &str = "GIT_ATTR_SOURCE";

/// git's empty tree.
pub(crate) const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// The variable that bounds git's search for a repository, set for Codex alone to the matter
/// folder's parent (Claude Code removes it from its own git's environment).
pub(crate) const CEILING_VAR: &str = "GIT_CEILING_DIRECTORIES";

/// Why the program cannot be used, as a word for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unusable {
    /// The desktop could not copy it.
    NotCopied,
    /// Nothing is there, or it cannot be read.
    Missing,
    /// Something other than a regular file is there: a folder or a link.
    NotAFile,
    /// It holds other bytes.
    Changed,
    /// Its folder holds something beside it.
    Crowded,
    /// A Windows Codex install's own folders hold a `git` program, or can't be checked
    /// (`codex_install.rs`).
    BesideCodex,
    /// None was named to the host, in a build for the lawyer ([`required`]).
    NotWired,
}

impl Unusable {
    /// The word a log line shows.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::NotCopied => "not_copied",
            Self::Missing => "missing",
            Self::NotAFile => "not_a_file",
            Self::Changed => "changed",
            Self::Crowded => "crowded",
            Self::BesideCodex => "beside_codex",
            Self::NotWired => "not_wired",
        }
    }
}

/// Whether file names fold ASCII case here: macOS and Windows disks do, so a `GIT.EXE` there is
/// the program (`clauseo_profile::tools`' sweep compares the same way).
const FOLD_CASE: bool = cfg!(any(windows, target_os = "macos"));

/// Whether `a` and `b` name the same file, ignoring ASCII case when `fold` ([`FOLD_CASE`]).
fn same_name(a: &std::ffi::OsStr, b: &std::ffi::OsStr, fold: bool) -> bool {
    if fold {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// Whether a build may launch an agent with a kind and no program named to the host: only a
/// test build (unit tests, or the `test-support` feature the integration tests use), whose
/// agents run with none.
pub(crate) const TEST_BUILD: bool = cfg!(any(test, feature = "test-support"));

/// What a launch of an agent with a kind needs before it starts, given the program named to the
/// host (`no_git`): it, or in a build for the lawyer (`test_build` false, [`TEST_BUILD`]) a
/// refusal when none was named.
pub(crate) fn required(
    no_git: Option<&NoGit>,
    test_build: bool,
) -> Result<Option<&NoGit>, Unusable> {
    match no_git {
        None if !test_build => Err(Unusable::NotWired),
        other => Ok(other),
    }
}

/// What a launch of an agent of `kind`, and each prompt and session call of it, checks: nothing
/// for an agent with no kind; else the program named to the host (`wired`), with a Codex
/// agent's install folder (from its settings `entry`) on Windows (`windows`), or in a build for
/// the lawyer (`test_build` false) the refusal when none was named ([`required`]).
pub(crate) fn check_for(
    kind: Option<AgentKind>,
    wired: Option<&NoGit>,
    entry: Option<&AgentLaunch>,
    (test_build, windows): (bool, bool),
) -> Result<Option<Check>, Unusable> {
    let Some(kind) = kind else {
        return Ok(None);
    };
    let Some(program) = required(wired, test_build)? else {
        return Ok(None);
    };
    let codex_install = (kind == AgentKind::Codex && windows)
        .then_some(entry)
        .flatten()
        .and_then(codex_install::install_of);
    Ok(Some(Check {
        no_git: program.clone(),
        codex_install,
    }))
}

/// What one check covers: the program, and for a Codex launch on Windows its install folder
/// (`codex_install.rs`).
#[derive(Debug, Clone)]
pub(crate) struct Check {
    /// The program named to the host.
    pub(crate) no_git: NoGit,
    /// The Codex install folder to search, on Windows.
    pub(crate) codex_install: Option<PathBuf>,
}

/// [`check`], then for a Windows Codex launch [`codex_install::beside_codex`] (logged with its
/// word): the program's folder, or why the agent must not start. At launch and again before
/// every prompt and session call, off the actor.
pub(crate) fn check_all(check_of: &Check) -> Result<PathBuf, Unusable> {
    let folder = check(&check_of.no_git)?;
    if let Some(install) = &check_of.codex_install
        && let Err(beside) = codex_install::beside_codex(install)
    {
        tracing::warn!(
            why = beside.word(),
            "a Codex install folder is not vouched for"
        );
        return Err(Unusable::BesideCodex);
    }
    Ok(folder)
}

/// The program's folder, once `no_git`'s program is a regular file (never a link) holding the
/// bytes it must, alone in its folder (its own name compared ignoring case on macOS and Windows)
/// but for names starting with `.`; else why not. Reads the
/// whole program: off the actor.
pub(crate) fn check(no_git: &NoGit) -> Result<PathBuf, Unusable> {
    let want = no_git.sha256.ok_or(Unusable::NotCopied)?;
    let program = &no_git.program;
    let kind = std::fs::symlink_metadata(program).map_err(|_gone| Unusable::Missing)?;
    if !kind.file_type().is_file() {
        return Err(Unusable::NotAFile);
    }
    let bytes = std::fs::read(program).map_err(|_gone| Unusable::Missing)?;
    if Sha256::digest(&bytes).as_slice() != want.as_slice() {
        return Err(Unusable::Changed);
    }
    let folder = program.parent().ok_or(Unusable::Missing)?;
    let name = program.file_name().ok_or(Unusable::Missing)?;
    let entries = std::fs::read_dir(folder).map_err(|_gone| Unusable::Missing)?;
    for entry in entries {
        let entry = entry.map_err(|_gone| Unusable::Missing)?;
        let other = entry.file_name();
        // The program's name in any case on macOS and Windows, as the desktop's sweep keeps it;
        // a name starting with `.` is the copy's own leftover, which no lookup of `git` finds.
        if !same_name(&other, name, FOLD_CASE) && !other.as_encoded_bytes().starts_with(b".") {
            return Err(Unusable::Crowded);
        }
    }
    Ok(folder.to_path_buf())
}

/// The refusal of a launch or call for `why`, logged with its word; its message names no reason.
pub(crate) fn refusal(why: Unusable) -> Error {
    tracing::warn!(
        why = why.word(),
        "the agent was not started or called: the program first on its PATH is {}",
        why.word()
    );
    // The reason stays in the log: the lawyer's line comes from the code alone.
    Error::refused(
        ErrorCode::AgentToolMissing,
        "A file Clauseo keeps for the agent isn't as Clauseo left it.",
    )
}

/// The variables a launch of `kind` gets, over any inherited or set ones: [`ATTR_SOURCE_VAR`]
/// for Claude and Codex, and for Codex [`CEILING_VAR`] naming `matter`'s parent (left out, with
/// a warning, when the parent cannot be written as one entry of the list). None for an agent
/// with no kind.
pub(crate) fn launch_env(kind: Option<AgentKind>, matter: &Path) -> BTreeMap<OsString, OsString> {
    let mut env = BTreeMap::new();
    if kind.is_none() {
        return env;
    }
    env.insert(OsString::from(ATTR_SOURCE_VAR), OsString::from(EMPTY_TREE));
    if kind == Some(AgentKind::Codex) {
        match matter.parent().map(|parent| std::env::join_paths([parent])) {
            Some(Ok(parent)) if !parent.is_empty() => {
                env.insert(OsString::from(CEILING_VAR), parent);
            }
            _ => tracing::warn!("the matter folder's parent cannot go in Codex's git variable"),
        }
    }
    env
}

/// For a Codex session whose `PATH` starts with the program's folder (`session_has_it`): its
/// `CODEX_CONFIG` gets `shell_environment_policy.set.PATH` holding the session's whole `PATH`,
/// so the model's shell commands find the program first too. A `PATH` that is not UTF-8, or a
/// `CODEX_CONFIG` that is not a JSON object, is left as it is, with a warning.
pub(crate) fn codex_shell_path(env: &mut BTreeMap<OsString, OsString>, session_has_it: bool) {
    if !session_has_it {
        return;
    }
    let path = env
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .and_then(|(_, value)| value.to_str())
        .map(str::to_owned);
    let Some(path) = path else {
        tracing::warn!("Codex's shells keep their PATH: the session's PATH is not UTF-8");
        return;
    };
    let name = OsString::from(super::codex::CONFIG_VAR);
    let config = env.get(&name).and_then(|value| value.to_str());
    let Some(Ok(mut config)) = config.map(serde_json::from_str::<serde_json::Map<_, _>>) else {
        tracing::warn!("Codex's shells keep their PATH: CODEX_CONFIG is not a JSON object");
        return;
    };
    super::codex::insert_dotted(
        &mut config,
        "shell_environment_policy.set.PATH",
        Value::String(path),
    );
    env.insert(name, OsString::from(Value::Object(config).to_string()));
}

pub(crate) mod codex_install;

#[cfg(test)]
mod tests;
