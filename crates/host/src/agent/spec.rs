//! The launch spec of an agent from its settings entry.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_process::LaunchSpec;
use clauseo_settings::AgentLaunch;

use crate::error::Error;

/// The spec for `agent`, run in `cwd` (the matter folder).
///
/// The env map holds the whole `inherited` environment (the host's own, read at launch), then the
/// agent's variables over it, then the profile hook's over those. The child's environment is the
/// same as with the agent's and hook's variables alone, since the inherited ones are what the child
/// inherits anyway; they are in the map because the process crate's redactor takes its secrets
/// only from the spec's env, by its own name rule, so every inherited key or token stays out of
/// the stderr tail. Never `Debug`-print the spec: it holds those secrets.
pub(crate) fn launch_spec(
    agent: &AgentLaunch,
    hook_env: BTreeMap<OsString, OsString>,
    inherited: impl Iterator<Item = (OsString, OsString)>,
    cwd: PathBuf,
) -> Result<LaunchSpec, Error> {
    let mut env: BTreeMap<OsString, OsString> = inherited.collect();
    env.extend(
        agent
            .env
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value))),
    );
    env.extend(hook_env);
    let args = agent.args.iter().map(OsString::from).collect();
    Ok(LaunchSpec::new(
        PathBuf::from(&agent.command),
        args,
        env,
        cwd,
    )?)
}
