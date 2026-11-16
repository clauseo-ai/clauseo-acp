//! A thread's agent launch itself (step 2's spawn, from `agents.rs`): the spec from the
//! settings, the profile hook and the host's switches, in the matter folder that was checked, and
//! a Codex agent's hooks seam.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use base64::Engine as _;
use clauseo_process::TreeWatch;
use clauseo_protocol::{AgentKind, ErrorCode, ThreadId};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::key_retry::{KeyRetry, own_login};
use super::super::launch_checks::Prepared;
use super::super::offline::probe::Target;
use super::{CodexAgent, path, start_failure};
use crate::agent::{self, AgentLink};
use crate::error::Error;

/// What a launch gives: the link, the folder it runs in, what a Codex launch recorded, a
/// Claude launch's plans folder, the matter folder's spellings for its stderr summary, a Claude
/// launch's home, the launch variables the data fence expands (`data_fence::launch_vars`), and
/// what the offline probe asks (`offline/probe/service.rs`).
pub(super) type Launched = (
    AgentLink,
    PathBuf,
    Option<CodexAgent>,
    Option<PathBuf>,
    Vec<String>,
    Option<PathBuf>,
    Vec<(String, String)>,
    Option<Target>,
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
    // The relay token and the browser proxy's password reach the agent's tree in files; its
    // stderr tail hides them all the same. So are the lawyer's MCP values and remote URLs, which
    // reach it in `session/new`.
    let mut redacted = super::super::launch_checks::relay::redacted(&prepared.run);
    redacted.extend(agent::loads::lawyer::redacted(&prepared.run.lawyer.servers));
    redacted.extend(
        prepared
            .run
            .egress
            .as_ref()
            .and_then(|egress| egress.secret())
            .into_iter()
            .flat_map(|credential| {
                // The password, and the `Proxy-Authorization: Basic` value it makes.
                let basic = format!("{}:{}", credential.user(), credential.password());
                [
                    credential.password().to_owned(),
                    base64::engine::general_purpose::STANDARD.encode(basic),
                ]
            }),
    );
    let folder = prepared.stored;
    let claude_home = prepared.claude_home.clone();
    let record = prepared
        .run
        .guard
        .as_ref()
        .map(|(file, id)| (file.clone(), id.hex()));
    let (mut forced, codex) = if let Some(prepared) = prepared.codex {
        let record = record
            .as_ref()
            .map(|(file, id)| (file.as_path(), id.as_str()));
        let hooks = codex_hooks(actor, record, &prepared.home);
        let guard = hooks.is_some();
        let forced = agent::codex::launch_env(&prepared.home, &settings.env, prepared.keys, hooks);
        let codex = CodexAgent {
            folder: prepared.folder,
            guard,
            key_retry: KeyRetry::default(),
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
    // Desktop's hook sets the base URL only from a plan (its `plan_env`).
    let planned = hook
        .iter()
        .any(|(name, value)| name.eq_ignore_ascii_case("ANTHROPIC_BASE_URL") && !value.is_empty());
    let session_folder = agent::session_folder(&folder);
    let matter_spellings = start_failure::spellings_at_launch(actor, &folder, &session_folder);
    // The offline probe asks only `chatgpt.com` for a login that is Clauseo's own.
    let own = codex.as_ref().is_some_and(|(_, login)| own_login(*login));
    // The key retry is armed by the hook's key and the launch's sign-in check.
    let codex = codex.map(|(mut codex, login)| {
        codex.key_retry = KeyRetry::at_launch(&hook, login);
        codex
    });
    let parts = path::launch_parts(actor, codex.is_some(), &prepared.run.path);
    let claude = AgentKind::of(&row.agent) == Some(AgentKind::Claude);
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
    // On Windows Claude's one shell is PowerShell (`agent/one_shell.rs`).
    if claude {
        agent::one_shell::apply_env(&mut env, agent::shell_path::Os::host());
    }
    let spec = agent::spec_of(settings, env, session_folder)
        .map_err(super::super::launch_checks::folder::gone_at_spawn)?
        .with_redacted(redacted);
    // Desktop refuses a firm endpoint with no key first (`agent.firm_key_needed`). This checks the
    // env the agent would get (the inherited one, the entry's, the hook's) with the `env` Claude
    // Code applies over it from its settings files and policy tiers, read by the prep, and holds
    // if desktop's refusal never reached the host or has not yet (a settings change lands in the
    // launch hook before desktop's `refusal::sync_all`).
    let env = spec
        .env()
        .iter()
        .map(|(name, value)| (name.as_os_str(), value.as_os_str()));
    if claude && clauseo_settings::own_signin_reaches_endpoint(env, &prepared.settings.env) {
        tracing::warn!(agent = %row.agent, planned, "a launch is refused: a custom endpoint, no key");
        // The firm's words only when a plan set the base URL; else the card's own.
        let code = if planned {
            ErrorCode::AgentFirmKeyNeeded
        } else {
            ErrorCode::AgentEndpointKeyNeeded
        };
        return Err(Error::refused(
            code,
            "a custom endpoint with no key saved would get Claude's own sign-in",
        ));
    }
    let plan_notes = claude
        .then(|| agent::plan_note::plans_folder(spec.env(), cfg!(windows)))
        .flatten();
    let fence_vars = agent::data_fence::launch_vars(spec.env());
    // The service and route the offline probe asks, from the env the agent gets, unless the
    // agent's own settings files move them (read by the prep).
    let kind = AgentKind::of(&row.agent);
    // The unit tests' `fake-agent` has no kind; a test names the one it plays.
    #[cfg(test)]
    let kind = kind.or(actor.turn_path.kind_for_tests);
    let moved = prepared.settings.moved;
    let probe = kind.map(|kind| Target::at_launch(kind, spec.env(), own).unless_moved(moved));
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
        probe,
    ))
}

/// THE CODEX HOOKS SEAM: the `hooks` object a Codex agent's `CODEX_CONFIG` carries, with its
/// `state` trust entry and `trusted_hash`: the one a test set (`HostConfig::with_codex_hooks`),
/// else [`codex_hook_config`](agent::loads::codex_hook_config) for the guard program the
/// Claude Code entry runs (`HostConfig::with_pre_write_guard`), so its `PreToolUse` holds one
/// group, the guard's, which records its refusals in the session's `record` file, with that
/// file's identity, for the lawyer (`updates/guard_blocks.rs`), and Codex's `home`, whose files
/// the guard refuses (`pre_write_guard::CODEX_HOME_FLAG`); none when the launch prep made
/// no record (a Claude session). `None` with no guard program, or (with a warning) one that
/// `codex_hook_config` cannot quote. It is read once per launch, which records on the agent
/// whether one went in (`Agent::guard`); with none the session start skips the guard's trust
/// check.
fn codex_hooks(
    actor: &Actor,
    record: Option<(&std::path::Path, &str)>,
    home: &std::path::Path,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    if let Some(hooks) = &actor.codex_hooks {
        return Some(hooks.clone());
    }
    // On Windows the console program beside the app, which PowerShell waits for.
    let program = agent::loads::codex_guard_program(actor.pre_write_guard.as_deref()?);
    let config = agent::loads::codex_hook_config_with_home(&program, record, Some(home));
    let Some(serde_json::Value::Object(hooks)) = config else {
        tracing::warn!(
            "the pre-write guard's path cannot go in Codex's hook command, so Codex runs without \
             the guard"
        );
        return None;
    };
    Some(hooks)
}
