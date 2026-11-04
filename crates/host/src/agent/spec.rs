//! The launch spec of an agent from its settings entry.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use clauseo_process::LaunchSpec;
use clauseo_settings::AgentLaunch;

use crate::error::Error;

/// The spec for `agent`, run in `cwd` (the matter folder).
///
/// The env map holds the `inherited` environment (the host's own, read at launch) minus every
/// `GIT_*` and `SSH_*` variable, the names in [`DROPPED_INHERITED`], and every name an agent,
/// hook or forced variable also names, then the
/// agent's variables minus those the hook or `forced` names, then the hook's minus those
/// `forced` names, then `forced`: what the host sets for the agent's kind (a Codex agent's
/// mode, home and config, `agent/codex.rs`), which nothing in the settings can override. Every
/// name in `dropped` (a Codex agent's `codex::DROPPED_VARS`) is left out wherever it comes
/// from (inherited, the agent's variables or the hook's), ignoring ASCII case. No
/// inherited `GIT_*` reaches any agent: a `GIT_DIR` or `GIT_WORK_TREE` the host was started
/// with would point the agent's git at another repository (agents never see a git repository
/// through the environment). No inherited `NO_BROWSER` or `CLAUDE_CODE_REMOTE` reaches one
/// either: each tells an agent it runs on a remote or headless machine, so it would skip the
/// browser sign-in on the lawyer's own desktop. Every inherited `SSH_*` is dropped on purpose,
/// the SSH agent's own (`SSH_AUTH_SOCK`, `SSH_AGENT_PID`, `SSH_ASKPASS`) included: the session
/// markers (`SSH_CONNECTION`, `SSH_CLIENT`, `SSH_TTY`) would skip the sign-in too, and the
/// agent's would hand an AI agent the lawyer's ssh keys, a way to send data out. No agent needs
/// remote git: Clauseo's checkpoints use git in-process (gix). Names are compared ignoring ASCII
/// case: on Windows `PATH` and `Path` are one variable and `Command::envs` keeps whichever comes
/// last in the map's order, so an inherited `Path` left beside the agent's `PATH` would win.
///
/// The map is the child's whole environment: the caller's launcher must clear the environment
/// (`SpawnOptions::clear_env`, as `HostConfig::new` requires), or the child inherits the host's
/// own and every drop above is undone. Clearing loses nothing else, since the map already holds
/// the rest of the inherited environment (`PATH`, `HOME`, and on Windows `SystemRoot`,
/// `PATHEXT`, `APPDATA` and the others Node and the agents need). The map also feeds the process
/// crate's redactor, which takes its secrets only from the spec's env, by its own name rule, so
/// every inherited key or token stays out of the stderr tail. Never `Debug`-print the spec: it
/// holds those secrets.
///
/// `path_prepend` (`HostConfig::with_agent_path_prepend`, stetdoc's folder) goes first on the
/// merged `PATH` ([`prepend_path`]).
pub(crate) fn launch_spec(
    agent: &AgentLaunch,
    mut hook_env: BTreeMap<OsString, OsString>,
    forced: BTreeMap<OsString, OsString>,
    dropped: &[&str],
    inherited: impl Iterator<Item = (OsString, OsString)>,
    cwd: PathBuf,
    path_prepend: Option<&Path>,
) -> Result<LaunchSpec, Error> {
    let drops = |name: &OsStr| dropped.iter().any(|drop| same(OsStr::new(drop), name));
    hook_env.retain(|name, _| !names(&forced, name) && !drops(name));
    let own: Vec<(OsString, OsString)> = agent
        .env
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .filter(|(name, _)| !names(&hook_env, name) && !names(&forced, name) && !drops(name))
        .collect();
    let mut env: BTreeMap<OsString, OsString> = inherited
        .filter(|(name, _)| {
            !is_dropped_inherited(name)
                && !drops(name)
                && !names(&hook_env, name)
                && !names(&forced, name)
                && !own.iter().any(|(own, _)| same(own, name))
        })
        .collect();
    env.extend(own);
    env.extend(hook_env);
    env.extend(forced);
    if let Some(folder) = path_prepend {
        prepend_path(&mut env, folder);
    }
    let args = agent.args.iter().map(OsString::from).collect();
    Ok(LaunchSpec::new(
        PathBuf::from(&agent.command),
        args,
        env,
        cwd,
    )?)
}

/// Puts `folder` first on `env`'s `PATH`: the one the merge kept (the hook's, else the agent's
/// settings, else the inherited one), found ignoring ASCII case and written back under its own
/// name (`Path` on Windows stays `Path`), or a new `PATH` holding only `folder`. A folder
/// that cannot be joined (it holds the separator) is left out with a warning.
#[expect(
    clippy::disallowed_methods,
    reason = "the agent's PATH value is split once, to put stetdoc's folder first; nothing is looked up on it"
)]
pub(crate) fn prepend_path(env: &mut BTreeMap<OsString, OsString>, folder: &Path) {
    let name = env
        .keys()
        .find(|key| same(key, OsStr::new("PATH")))
        .cloned()
        .unwrap_or_else(|| OsString::from("PATH"));
    let rest = env.get(&name).cloned().unwrap_or_default();
    let parts = std::iter::once(folder.to_path_buf())
        .chain(std::env::split_paths(&rest).filter(|part| !part.as_os_str().is_empty()));
    match std::env::join_paths(parts) {
        Ok(joined) => {
            env.insert(name, joined);
        }
        Err(error) => tracing::warn!(
            folder = %folder.display(),
            %error,
            "the stetdoc folder cannot go on the agent's PATH"
        ),
    }
}

/// The prefixes of the inherited variables no agent gets: `GIT_*` would point its git at another
/// repository, `SSH_*` would tell it it runs over SSH or lend it the lawyer's ssh keys.
/// `crates/limits/src/readers/env.rs` keeps the same lists for the plan readers.
pub(crate) const DROPPED_INHERITED_PREFIXES: [&str; 2] = ["GIT_", "SSH_"];

/// The inherited variables no agent gets: each tells it it runs headless or remote, so it would
/// skip the browser sign-in.
pub(crate) const DROPPED_INHERITED: [&str; 2] = ["NO_BROWSER", "CLAUDE_CODE_REMOTE"];

/// Whether an inherited `name` stays out of every agent's environment: it has a prefix in
/// [`DROPPED_INHERITED_PREFIXES`] or is in [`DROPPED_INHERITED`], ignoring ASCII case (Windows
/// does).
fn is_dropped_inherited(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    DROPPED_INHERITED_PREFIXES.iter().any(|prefix| {
        bytes
            .get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes()))
    }) || DROPPED_INHERITED
        .iter()
        .any(|drop| same(OsStr::new(drop), name))
}

/// Whether `map` holds `name`, ignoring ASCII case.
fn names(map: &BTreeMap<OsString, OsString>, name: &OsStr) -> bool {
    map.keys().any(|key| same(key, name))
}

/// One variable name on every OS: Windows ignores the case of env names.
fn same(a: &OsStr, b: &OsStr) -> bool {
    a.eq_ignore_ascii_case(b)
}
