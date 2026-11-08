//! The variables a launch forces for the agent's kind with the matter's state folder (R17 PR-C):
//! Codex's ([`codex`]) and Claude's ([`claude`]), and a Codex agent's own state ([`of`]).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use clauseo_protocol::AgentId;
use serde_json::{Map, Value};

use super::super::super::Actor;
use super::super::key_retry::KeyRetry;
use super::super::launch_checks::{CodexPrepared, RunFolders, Sandboxed};
use super::{CodexAgent, codex_hooks};
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
        match value {
            Some(value) => {
                forced.insert(name.clone(), value.clone());
            }
            None => {
                forced.remove(name);
                removed.extend(name.to_str().map(str::to_owned));
            }
        }
    }
    removed
}
