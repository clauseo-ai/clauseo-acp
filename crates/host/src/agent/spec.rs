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
/// name an agent or hook variable also names, then the agent's variables minus those the hook
/// names, then the hook's. Names are compared ignoring ASCII case: on Windows `PATH` and `Path`
/// are one variable and `Command::envs` keeps whichever comes last in the map's order, so an
/// inherited `Path` left beside the agent's `PATH` would win. The child's environment is the
/// same as with the agent's and hook's variables alone, since the inherited ones are what the
/// child inherits anyway; they are in the map because the process crate's redactor takes its
/// secrets only from the spec's env, by its own name rule, so every inherited key or token stays
/// out of the stderr tail. Never `Debug`-print the spec: it holds those secrets.
pub(crate) fn launch_spec(
    agent: &AgentLaunch,
    hook_env: BTreeMap<OsString, OsString>,
    inherited: impl Iterator<Item = (OsString, OsString)>,
    cwd: PathBuf,
) -> Result<LaunchSpec, Error> {
    let own: Vec<(OsString, OsString)> = agent
        .env
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .filter(|(name, _)| !names(&hook_env, name))
        .collect();
    let mut env: BTreeMap<OsString, OsString> = inherited
        .filter(|(name, _)| !names(&hook_env, name) && !own.iter().any(|(own, _)| same(own, name)))
        .collect();
    env.extend(own);
    env.extend(hook_env);
    let args = agent.args.iter().map(OsString::from).collect();
    Ok(LaunchSpec::new(
        PathBuf::from(&agent.command),
        args,
        env,
        cwd,
    )?)
}

/// Whether `map` holds `name`, ignoring ASCII case.
fn names(map: &BTreeMap<OsString, OsString>, name: &OsStr) -> bool {
    map.keys().any(|key| same(key, name))
}

/// One variable name on every OS: Windows ignores the case of env names.
fn same(a: &OsStr, b: &OsStr) -> bool {
    a.eq_ignore_ascii_case(b)
}
