//! The variables a launch forces for the agent's kind with the matter's state folder (R17 PR-C):
//! Codex's ([`codex`]) and Claude's ([`claude`]), and a Codex agent's own state ([`of`]); a
//! jailed launch's own over them ([`jailed`]), and the other sources of its spec ([`sources`]).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::AgentId;
use clauseo_settings::AgentLaunch;
use serde_json::{Map, Value};

use super::super::super::Actor;
use super::super::key_retry::KeyRetry;
use super::super::launch_checks::{CodexPrepared, RunFolders, Sandboxed};
use super::CodexAgent;
use crate::agent;
use crate::agent::login_link::Checked;

/// A launch's forced variables, and for a Codex agent (`codex`) its state with what the sign-in
/// hook found at the launch.
pub(super) type Forced = (
    BTreeMap<OsString, OsString>,
    Option<(CodexAgent, Option<Checked>)>,
);

/// The forced variables of `agent`'s launch: Codex's when `codex` (its preparation), else
/// Claude's, from the session's folders `run` and Claude's login home `claude_home`.
pub(super) fn of(
    actor: &Actor,
    agent: &AgentId,
    settings_env: &BTreeMap<String, String>,
    codex: Option<CodexPrepared>,
    run: &RunFolders,
    claude_home: Option<&Path>,
) -> Forced {
    let Some(prepared) = codex else {
        return (
            claude(actor, agent, run.state.as_deref(), claude_home),
            None,
        );
    };
    let hooks = codex_hooks(actor);
    let guard = hooks.is_some();
    let forced = self::codex(
        &prepared.state,
        &run.tmp(),
        settings_env,
        prepared.keys,
        hooks,
    );
    let codex = CodexAgent {
        folder: prepared.folder,
        guard,
        key_retry: KeyRetry::default(),
    };
    (forced, Some((codex, prepared.login)))
}

/// A Codex launch's forced variables (`agent::codex::launch_env`, `state` its `CODEX_HOME`),
/// with the shell's temp folder the session's run folder's (`run_tmp`, `RunFolders::tmp`),
/// never the state folder's `tmp`, whose `arg0` links Codex puts first on `PATH` and runs
/// unsandboxed.
pub(super) fn codex(
    state: &Path,
    run_tmp: &Path,
    settings_env: &BTreeMap<String, String>,
    keys: Map<String, Value>,
    hooks: Option<Map<String, Value>>,
) -> BTreeMap<OsString, OsString> {
    let mut forced = agent::codex::launch_env(state, settings_env, keys, hooks);
    forced.insert(
        OsString::from(agent::codex::TMPDIR_VAR),
        run_tmp.as_os_str().to_owned(),
    );
    forced
}

/// A Claude launch's forced variables: its state folder (`state`), which goes in before
/// `plans_folder` reads the merged map; its login store is the login home (`claude_home`),
/// or the lawyer's default store while desktop says `agent`'s login is reused. None without
/// a state folder or a login home.
///
/// The reused store `""` is the lawyer's default store (on macOS their Keychain). Right at tier
/// C, all PR-C has: no jail stands between the agent and that store. The tier A and B rule
/// (`SignedOut { found: ClaudeSandbox }`, S18) lands with the jail in R17 PR-D.
pub(super) fn claude(
    actor: &Actor,
    agent: &AgentId,
    state: Option<&Path>,
    claude_home: Option<&Path>,
) -> BTreeMap<OsString, OsString> {
    let reused = actor.wiring.sign_in.reused.contains(agent);
    let store = claude_home.filter(|_| !reused);
    state
        .filter(|_| claude_home.is_some())
        .map(|state| agent::home::claude_launch_env(state, store))
        .unwrap_or_default()
}

/// A jailed launch's variables (`Sandboxed::env`: the session proxy's, which carry its token, and
/// the CA bundle's) put over `forced`, so they win over every other source; the names it
/// removes (`ALL_PROXY`, `all_proxy`) come back to be dropped from every source. Tier C
/// (`sandbox` none) changes nothing.
pub(super) fn jailed(
    forced: &mut BTreeMap<OsString, OsString>,
    sandbox: Option<&Sandboxed>,
) -> Vec<String> {
    let mut removed = Vec::new();
    for (name, value) in sandbox.map(|jail| jail.env.as_slice()).unwrap_or_default() {
        if let Some(value) = value {
            forced.insert(name.clone(), value.clone());
        } else {
            forced.remove(name);
            removed.extend(name.to_str().map(str::to_owned));
        }
    }
    removed
}

/// A launch's other sources for `launch_spec`: the agent's settings, the hook's variables with the
/// host's switch (`loads::launch_env`), the inherited environment (`HostWiring::inherited_env`,
/// else this process's) and the folder put first on `PATH` (stetdoc's).
pub(super) type Sources = (
    AgentLaunch,
    BTreeMap<OsString, OsString>,
    Vec<(OsString, OsString)>,
    Option<PathBuf>,
);

/// [`Sources`] for `agent`'s launch with `settings`: as they are at tier C; by
/// `agent::jailed_env`'s rules when `jailed`, with no folder for `PATH` (stetdoc's is among
/// the policy's runtime folders, which make the jailed `PATH`).
pub(super) fn sources(
    actor: &Actor,
    agent: &AgentId,
    settings: &AgentLaunch,
    jailed: bool,
) -> Sources {
    let hook = actor.profile.launch_env(agent);
    let inherited: Vec<_> = match &actor.wiring.inherited_env {
        Some(env) => env.clone(),
        None => std::env::vars_os().collect(),
    };
    if !jailed {
        let prepend = actor.wiring.path_prepend.clone();
        return (
            settings.clone(),
            agent::loads::launch_env(hook),
            inherited,
            prepend,
        );
    }
    let jailed = agent::jailed_env::sources(inherited, settings, hook);
    let hook = agent::loads::launch_env(jailed.hook);
    (jailed.settings, hook, jailed.inherited, None)
}

/// THE CODEX HOOKS SEAM: the `hooks` object a Codex agent's `CODEX_CONFIG` carries, with its
/// `state` trust entry and `trusted_hash`: the one a test set (`HostConfig::with_codex_hooks`),
/// else [`codex_hook_config`](agent::loads::codex_hook_config) for the guard program the
/// Claude Code entry runs (`HostConfig::with_pre_write_guard`), so its `PreToolUse` holds one
/// group, the guard's. `None` with no guard program, or (with a warning) one that
/// `codex_hook_config` cannot quote. It is read once per launch, which records on the agent
/// whether one went in (`Agent::guard`); with none the session start skips the guard's trust
/// check.
fn codex_hooks(actor: &Actor) -> Option<Map<String, Value>> {
    if let Some(hooks) = &actor.codex_hooks {
        return Some(hooks.clone());
    }
    let program = actor.pre_write_guard.as_deref()?;
    let Some(Value::Object(hooks)) = agent::loads::codex_hook_config(program) else {
        tracing::warn!(
            "the pre-write guard's path cannot go in Codex's hook command, so Codex runs without \
             the guard"
        );
        return None;
    };
    Some(hooks)
}
