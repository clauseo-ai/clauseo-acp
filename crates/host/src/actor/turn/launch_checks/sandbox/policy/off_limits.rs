//! The lawyer's folders no runtime folder of a jailed launch may sit in besides their personal
//! folders and the matters ([`super::runtime`]), as the jail would let the agent read there:
//! their Claude Code and Codex homes (`~/.claude`, `~/.codex`, and a custom `CLAUDE_CONFIG_DIR`
//! or `CODEX_HOME` from their environment, [`Grants::agent_homes`]) and their credential stores.

use std::path::{Path, PathBuf};

use super::super::super::Prepared;
use super::{Grants, launch_failed};
use crate::error::Error;

/// The lawyer's own agent homes (`home/<name>`), holding their Claude Code and Codex logins.
const AGENT_HOMES: [&str; 2] = [".claude", ".codex"];

/// The lawyer's credential stores (`home/<names>`): SSH keys, AWS', `GnuPG`'s and `gcloud`'s.
const CREDENTIAL_STORES: [&[&str]; 4] = [&[".ssh"], &[".aws"], &[".gnupg"], &[".config", "gcloud"]];

/// Whether names also compare by their lowercase on top of `clauseo_sandbox::path_under`, which
/// folds on macOS only: on Windows, as NTFS looks names up case-insensitively.
pub(super) const FOLD_CASE: bool = cfg!(windows);

/// Refuses the launch when one of `runtime` sits in the lawyer's Claude Code or Codex home, a
/// credential store, their Documents, Desktop or Downloads, or a matter of the registry; names
/// compared by [`within`] with `fold` ([`FOLD_CASE`] at a launch). File system calls: off the
/// actor.
pub(super) fn refuse(
    grants: &Grants,
    prepared: &Prepared,
    runtime: &[PathBuf],
    fold: bool,
) -> Result<(), Error> {
    if inside(runtime, agent_homes(grants), fold) {
        return Err(launch_failed(
            "The sandbox couldn't start the agent: its program is in Claude Code's or Codex's own \
             folder. Install it elsewhere.",
        ));
    }
    if inside(runtime, credential_stores(grants), fold) {
        return Err(launch_failed(
            "The sandbox couldn't start the agent: its program is in a folder holding your SSH, \
             GnuPG or cloud keys. Install it elsewhere.",
        ));
    }
    if inside(runtime, (grants.personal)(grants.home.as_deref()), fold) {
        return Err(launch_failed(
            "The sandbox couldn't start the agent: its program is in the Documents, Desktop or \
             Downloads folder. Install it elsewhere.",
        ));
    }
    let mut matters = grants.others.clone();
    matters.push(prepared.stored.clone());
    if inside(runtime, matters, fold) {
        return Err(launch_failed(
            "The sandbox couldn't start the agent: its program is in a matter's folder. Install \
             it elsewhere.",
        ));
    }
    Ok(())
}

/// The lawyer's Claude Code and Codex homes: `~/.claude`, `~/.codex` and their custom ones.
pub(super) fn agent_homes(grants: &Grants) -> Vec<PathBuf> {
    let defaults = grants
        .home
        .iter()
        .flat_map(|home| AGENT_HOMES.map(|name| home.join(name)));
    defaults.chain(grants.agent_homes.iter().cloned()).collect()
}

/// The lawyer's credential stores under their home folder.
pub(super) fn credential_stores(grants: &Grants) -> Vec<PathBuf> {
    grants
        .home
        .iter()
        .flat_map(|home| {
            CREDENTIAL_STORES.map(|names| {
                names
                    .iter()
                    .fold(home.clone(), |path, name| path.join(name))
            })
        })
        .collect()
}

/// Whether any of `runtime` (canonical) is in any of `outers`, each compared canonical when it
/// resolves and as given when not, by [`within`] with `fold`. File system calls: off the actor.
pub(super) fn inside(runtime: &[PathBuf], outers: Vec<PathBuf>, fold: bool) -> bool {
    let outers: Vec<PathBuf> = outers
        .into_iter()
        .map(|outer| dunce::canonicalize(&outer).unwrap_or(outer))
        .collect();
    runtime
        .iter()
        .any(|folder| outers.iter().any(|outer| within(folder, outer, fold)))
}

/// Whether `path` is `outer` or under it: by `clauseo_sandbox::path_under`, then with `fold` by
/// each name's lowercase.
pub(super) fn within(path: &Path, outer: &Path, fold: bool) -> bool {
    if clauseo_sandbox::path_under(path, outer) {
        return true;
    }
    if !fold {
        return false;
    }
    let key = |name: std::path::Component<'_>| name.as_os_str().to_string_lossy().to_lowercase();
    let mut names = path.components();
    outer
        .components()
        .all(|want| names.next().is_some_and(|got| key(got) == key(want)))
}
