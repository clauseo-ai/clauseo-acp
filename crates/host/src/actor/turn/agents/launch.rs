//! A thread's agent launch itself (step 2's spawn, from `agents.rs`): the spec from the
//! settings, the profile hook and the host's switches, in the matter folder that was checked, and
//! a Codex agent's hooks seam.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use clauseo_process::TreeWatch;
use clauseo_protocol::{AgentKind, ErrorCode, ThreadId};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::key_retry::KeyRetry;
use super::super::launch_checks::Prepared;
use super::{CodexAgent, path, start_failure};
use crate::agent::{self, AgentLink};
use crate::error::Error;

/// What a launch gives: the link, the folder it runs in, what a Codex launch recorded, a
/// Claude launch's plans folder, the matter folder's spellings for its stderr summary, a Claude
/// launch's home, and the launch variables the data fence expands (`data_fence::launch_vars`).
pub(super) type Launched = (
    AgentLink,
    PathBuf,
    Option<CodexAgent>,
    Option<PathBuf>,
    Vec<String>,
    Option<PathBuf>,
    Vec<(String, String)>,
);

/// The launch itself: the spec from the settings (inherited environment, the agent's
/// variables, then the profile hook's with the host's switches over them, then what the host
/// forces for the agent's kind, with stetdoc's and the hook's folders first on `PATH`), in the
/// matter folder that was checked (`prepared`). A Codex agent's `.codex` refusal (#184), its
/// home and its `config.toml` are done by then.
pub(super) fn launch(
    actor: &Actor,
    thread: ThreadId,
    prepared: Prepared,
    tree: &mut Option<TreeWatch>,
) -> Result<Launched, Error> {
    let row = &actor.check_thread(thread)?.row;
    let settings = actor.effective.agent(row.agent.as_str()).ok_or_else(|| {
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            format!("no agent {} in the settings", row.agent),
        )
    })?;
    // The relay token reaches the agent in a file; its stderr tail hides it all the same. So
    // are the lawyer's MCP values and remote URLs, which reach it in `session/new`.
    let mut redacted = super::super::launch_checks::relay::redacted(&prepared.run);
    redacted.extend(agent::loads::lawyer::redacted(&prepared.run.lawyer.servers));
    let folder = prepared.stored;
    let claude_home = prepared.claude_home.clone();
    let (mut forced, codex) = if let Some(prepared) = prepared.codex {
        let hooks = codex_hooks(actor);
        let guard = hooks.is_some();
        let forced = agent::codex::launch_env(&prepared.home, &settings.env, prepared.keys, hooks);
        let codex = CodexAgent {
            folder: prepared.folder,
            guard,
            key_retry: KeyRetry::default(),
            api_key: false,
        };
        (forced, Some((codex, prepared.login)))
    } else {
        // A Claude agent's home goes in before `plans_folder` reads the merged map below.
        let forced = prepared
            .claude_home
            .as_deref()
            .map(agent::home::claude_launch_env);
        (forced.unwrap_or_default(), None)
    };
    // Every agent's temp folder is its session's own, gone with the run folder.
    let short = prepared.run.claude_tmp.as_deref();
    let temp = agent::temp::session_env(&prepared.run.root, short, cfg!(windows));
    forced.extend(temp);
    let hook = agent::loads::launch_env(actor.profile.launch_env(&row.agent));
    let session_folder = agent::session_folder(&folder);
    let matter_spellings = start_failure::spellings_at_launch(actor, &folder, &session_folder);
    // The key retry is armed by the hook's key and the launch's sign-in check.
    let codex = codex.map(|(mut codex, login)| {
        codex.key_retry = KeyRetry::at_launch(&hook, login);
        codex
    });
    let parts = path::launch_parts(actor, codex.is_some(), &prepared.run.path);
    // Tier C (no jail) leaves the agent's environment alone: no session proxy variables here
    // (`ProxyHandle::env`); only the browser entry reaches the proxy, advisory without a jail.
    let mut env = agent::launch_env(
        settings,
        super::super::profile_loads::with_shell_env(actor, &prepared.run, hook),
        forced,
        &parts.dropped,
        parts.inherited.into_iter(),
        &parts.path,
    );
    // The agent's shells get the same `PATH` the session does (`agent/shell_path.rs`).
    agent::shell_path::apply(&mut env, &prepared.run.path);
    let spec = agent::spec_of(settings, env, session_folder)
        .map_err(super::super::launch_checks::folder::gone_at_spawn)?
        .with_redacted(redacted);
    let claude = AgentKind::of(&row.agent) == Some(AgentKind::Claude);
    let plan_notes = claude
        .then(|| agent::plan_note::plans_folder(spec.env(), cfg!(windows)))
        .flatten();
    let fence_vars = agent::data_fence::launch_vars(spec.env());
    // Whether Codex got a key decides the host its offline probe asks (`offline/probe.rs`).
    let codex = codex.map(|codex| CodexAgent {
        api_key: key_passed(spec.env()),
        ..codex
    });
    let post = actor.post.clone();
    let post: Rc<dyn Fn(agent::AgentInput)> = Rc::new(move |input| {
        // The actor may be gone by then.
        let _gone = post.try_send(Input::Agent { thread, input });
    });
    let link = agent::start(
        &actor.spawner,
        actor.launcher.as_ref(),
        &spec,
        Arc::clone(&actor.clock),
        post,
        tree,
    )?;
    Ok((
        link,
        folder,
        codex,
        plan_notes,
        matter_spellings,
        claude_home,
        fence_vars,
    ))
}

/// THE CODEX HOOKS SEAM: the `hooks` object a Codex agent's `CODEX_CONFIG` carries, with its
/// `state` trust entry and `trusted_hash`: the one a test set (`HostConfig::with_codex_hooks`),
/// else [`codex_hook_config`](agent::loads::codex_hook_config) for the guard program the
/// Claude Code entry runs (`HostConfig::with_pre_write_guard`), so its `PreToolUse` holds one
/// group, the guard's. `None` with no guard program, or (with a warning) one that
/// `codex_hook_config` cannot quote. It is read once per launch, which records on the agent
/// whether one went in (`Agent::guard`); with none the session start skips the guard's trust
/// check.
fn codex_hooks(actor: &Actor) -> Option<serde_json::Map<String, serde_json::Value>> {
    if let Some(hooks) = &actor.codex_hooks {
        return Some(hooks.clone());
    }
    let program = actor.pre_write_guard.as_deref()?;
    let Some(serde_json::Value::Object(hooks)) = agent::loads::codex_hook_config(program) else {
        tracing::warn!(
            "the pre-write guard's path cannot go in Codex's hook command, so Codex runs without \
             the guard"
        );
        return None;
    };
    Some(hooks)
}

/// Whether a Codex launch's environment passes a key: `CODEX_API_KEY` set and not empty (the
/// variable the profile hook launches a key in, `key_retry.rs`), its name in any case, as
/// Windows reads it.
fn key_passed(env: &std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>) -> bool {
    env.iter().any(|(name, value)| {
        name.to_string_lossy().eq_ignore_ascii_case("CODEX_API_KEY") && !value.is_empty()
    })
}
