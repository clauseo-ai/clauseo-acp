//! What a launch's spec takes beside the settings and the hook's variables: the folders first
//! on its `PATH`, the variables it drops, and the environment it inherits.

use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_protocol::AgentId;

use super::super::super::Actor;
use crate::agent;

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

/// The `PATH` folders, dropped variables and inherited environment of `agent`'s launch;
/// `codex` when it is a Codex agent, `tools` the shell browser's folder for a session that has
/// it. Stetdoc's folder comes first, then `tools` (so no `agent-browser` in the Python
/// runtime's folders shadows Clauseo's), then the hook's, then the user's `PATH`
/// (`agent::session_path`).
pub(super) fn launch_parts(
    actor: &Actor,
    agent: &AgentId,
    codex: bool,
    tools: Option<PathBuf>,
) -> LaunchParts {
    let (hook, hook_path) = agent::session_path(None, actor.profile.agent_path(agent));
    let path = actor
        .wiring
        .path_prepend
        .iter()
        .cloned()
        .chain(tools)
        .chain(hook)
        .collect();
    let mut dropped = Vec::new();
    if codex {
        dropped.extend(agent::codex::DROPPED_VARS);
    }
    if hook_path {
        dropped.extend(agent::PYTHON_VARS);
    }
    let inherited = match &actor.wiring.inherited_env {
        Some(env) => env.clone(),
        None => std::env::vars_os().collect(),
    };
    LaunchParts {
        path,
        dropped,
        inherited,
    }
}
