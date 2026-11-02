//! The launch spec of an agent from its settings entry.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use clauseo_process::LaunchSpec;
use clauseo_settings::AgentLaunch;

use crate::error::Error;

/// The spec for `agent`, run in `cwd` (the matter folder).
///
/// The env map holds the `inherited` environment (the host's own, read at launch) minus every
/// `GIT_*` variable and every name an agent, hook or forced variable also names, then the
/// agent's variables minus those the hook or `forced` names, then the hook's minus those
/// `forced` names, then `forced`: what the host sets for the agent's kind (a Codex agent's
/// mode, home and config, `agent/codex.rs`), which nothing in the settings can override. Every
/// name in `dropped` (a Codex agent's `codex::DROPPED_VARS`) is left out wherever it comes
/// from (inherited, the agent's variables or the hook's), ignoring ASCII case. No
/// inherited `GIT_*` reaches any agent: a `GIT_DIR` or `GIT_WORK_TREE` the host was started
/// with would point the agent's git at another repository (agents never see a git repository
/// through the environment). Names are compared ignoring ASCII case: on Windows `PATH` and `Path`
/// are one variable and `Command::envs` keeps whichever comes last in the map's order, so an
/// inherited `Path` left beside the agent's `PATH` would win. The child's environment is the
/// same as with the agent's and hook's variables alone, since the inherited ones are what the
/// child inherits anyway; they are in the map because the process crate's redactor takes its
/// secrets only from the spec's env, by its own name rule, so every inherited key or token stays
/// out of the stderr tail. Never `Debug`-print the spec: it holds those secrets.
pub(crate) fn launch_spec(
    agent: &AgentLaunch,
    mut hook_env: BTreeMap<OsString, OsString>,
    forced: BTreeMap<OsString, OsString>,
    dropped: &[&str],
    inherited: impl Iterator<Item = (OsString, OsString)>,
    cwd: PathBuf,
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
            !is_git(name)
                && !drops(name)
                && !names(&hook_env, name)
                && !names(&forced, name)
                && !own.iter().any(|(own, _)| same(own, name))
        })
        .collect();
    env.extend(own);
    env.extend(hook_env);
    env.extend(forced);
    let args = agent.args.iter().map(OsString::from).collect();
    Ok(LaunchSpec::new(
        PathBuf::from(&agent.command),
        args,
        env,
        cwd,
    )?)
}

/// Whether `name` is a `GIT_*` variable, ignoring ASCII case (Windows does).
fn is_git(name: &OsStr) -> bool {
    name.as_encoded_bytes()
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"GIT_"))
}

/// Whether `map` holds `name`, ignoring ASCII case.
fn names(map: &BTreeMap<OsString, OsString>, name: &OsStr) -> bool {
    map.keys().any(|key| same(key, name))
}

/// One variable name on every OS: Windows ignores the case of env names.
fn same(a: &OsStr, b: &OsStr) -> bool {
    a.eq_ignore_ascii_case(b)
}
