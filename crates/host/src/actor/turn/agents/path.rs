//! What a launch's spec takes beside the settings and the hook's variables: the folders first
//! on its `PATH`, the variables it drops, and the environment it inherits.

use std::ffi::OsString;
use std::path::PathBuf;

use super::super::super::Actor;
use crate::agent;
use crate::agent::shell_path::SessionPath;

/// [`launch_parts`]' answer.
pub(super) struct LaunchParts {
    /// Stetdoc's folder, the shell browser's tools folder, then the profile hook's (Clauseo's
    /// Python runtime), first on `PATH`.
    pub(super) path: Vec<PathBuf>,
    /// The variables dropped wherever they come from: a Codex agent's own list, and with the
    /// hook's folders on `PATH`, `PYTHONHOME` and `PYTHONPATH`.
    pub(super) dropped: Vec<&'static str>,
    /// The environment the agent inherits: the wiring's, else the host's own.
    pub(super) inherited: Vec<(OsString, OsString)>,
}

/// The `PATH` folders, dropped variables and inherited environment of a launch; `codex` when
/// it is a Codex agent. The folders are the ones the launch prep found (`session`, from
/// `launch_checks::paths::PathPrep`), the same ones its shells get (`agent/shell_path.rs`):
/// stetdoc's first, then the shell browser's tools for a session that has them (so no
/// `agent-browser` in the Python runtime's folders shadows Clauseo's), then the hook's, then
/// the user's `PATH`.
pub(super) fn launch_parts(actor: &Actor, codex: bool, session: &SessionPath) -> LaunchParts {
    let mut dropped = Vec::new();
    if codex {
        dropped.extend(agent::codex::DROPPED_VARS);
    }
    if session.hook_path {
        dropped.extend(agent::PYTHON_VARS);
    }
    let inherited = match &actor.wiring.inherited_env {
        Some(env) => env.clone(),
        None => std::env::vars_os().collect(),
    };
    LaunchParts {
        path: session.dirs.clone(),
        dropped,
        inherited,
    }
}
