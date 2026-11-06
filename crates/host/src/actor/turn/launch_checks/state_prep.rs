//! The matter's state folder in a launch (`agent/state_folder.rs`, R17 PR-C), on the launch-prep
//! thread: Codex's and Claude's preparation with it, and the reused Codex login's link, made once
//! the sign-in hook has answered.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::{AgentKind, ThreadId};

use super::CodexPrepared;
use crate::actor::Actor;
use crate::agent::codex;
use crate::agent::folder_support::{Answer, FolderCheck};
use crate::agent::login_link::{LoginLink, Seen};
use crate::agent::state_folder;
use crate::error::Error;
use crate::log_file::Cause;

/// A Codex launch's preparation on the launch-prep thread: `codex::prepare_launch` for the
/// login home `home`, then the matter's state folder `state` made, cleaned and given its
/// `config.toml` (not cleaned while another session runs there, `StateUse::in_use`), a stale
/// link removed first when `StateUse::drop_stale`, and on Clauseo's
/// own login (not `reused`) its `auth.json` linked.
pub(super) fn prepare_codex(
    check: &FolderCheck,
    stored: &Path,
    data: &Path,
    home: PathBuf,
    state: PathBuf,
    reused: bool,
    state_use: StateUse,
) -> Result<(CodexPrepared, Answer), Error> {
    let (folder, support) = codex::prepare_launch(check, stored, data, &home)?;
    if state_use.in_use {
        state_folder::prepare_in_use(data, &state, AgentKind::Codex)?;
    } else {
        state_folder::prepare(data, &state, AgentKind::Codex)?;
    }
    if state_use.drop_stale {
        state_folder::remove_stale_link(&state).map_err(|error| {
            tracing::warn!(error_kind = ?error.kind(), "a stale login link was not removed");
            Error::refused(
                clauseo_protocol::ErrorCode::AgentLaunchFailed,
                "Clauseo couldn't prepare Codex's folder.",
            )
        })?;
    }
    codex::write_home_config(&state)?;
    // A key login gets no login file; Clauseo's own login is linked now; a reused one once the
    // hook has answered (`link_reused_after`).
    let link = if state_use.key_login {
        Some(unlink_for_key(&state, state_use.in_use))
    } else {
        (!reused).then(|| state_folder::link_own(&home, &state).map_err(|error| error.refusal()))
    };
    let prepared = CodexPrepared {
        folder,
        home,
        state,
        keys: serde_json::Map::new(),
        login: None,
        link,
    };
    Ok((prepared, support))
}

/// A Codex key login's state folder: no `auth.json`, so Codex can use only the key. One left by
/// an earlier account login is removed, but not while another session of the agent runs in the
/// matter (`in_use`): that session holds it, so it stays and is logged, and the next launch
/// with no other session running removes it.
fn unlink_for_key(state: &Path, in_use: bool) -> Result<state_folder::StateLink, Error> {
    if in_use {
        if state.join(state_folder::LOGIN_FILE).exists() {
            tracing::info!("a key login's matter keeps a login link a running session holds");
        }
        return Ok(state_folder::StateLink::NoLogin);
    }
    state_folder::remove_stale_link(state).map_err(|error| {
        tracing::warn!(error_kind = ?error.kind(), "a key login's matter link was not removed");
        Error::refused(
            clauseo_protocol::ErrorCode::AgentLaunchFailed,
            "Clauseo couldn't prepare Codex's folder.",
        )
    })?;
    Ok(state_folder::StateLink::NoLogin)
}

/// A Claude launch's folders on the launch-prep thread: its login home `home` made, with
/// Claude Code's user settings (a failure is a warning), then the matter's state folder
/// `state` made, cleaned (not while another session runs there, `in_use`) and given its
/// `settings.json` (a failure fails the launch).
pub(super) fn prepare_claude(
    data: &Path,
    home: &Path,
    state: Option<&Path>,
    in_use: bool,
) -> Result<(), Error> {
    codex::make_agent_home(data, home, "Claude Code")?;
    if let Err(error) = crate::agent::home::write_claude_settings(home) {
        tracing::warn!(error = %Cause(&error), "Claude's home settings were not written");
    }
    let Some(state) = state else {
        return Ok(());
    };
    if in_use {
        state_folder::prepare_in_use(data, state, AgentKind::Claude)?;
    } else {
        state_folder::prepare(data, state, AgentKind::Claude)?;
    }
    crate::agent::home::write_state_settings(state).map_err(|error| {
        tracing::warn!(
            error = %Cause(&error),
            "Claude's state folder settings were not written"
        );
        Error::refused(
            clauseo_protocol::ErrorCode::AgentLaunchFailed,
            "Clauseo couldn't prepare Claude Code's folder.",
        )
    })
}

/// While the login is reused: links the matter's state folder to the login home's `auth.json`
/// off the actor (`state_folder::link_reused`), only when the sign-in hook found the home's
/// link holding (or no hook is set). A hook that found anything else leaves the state folder
/// as it is: the split check refuses the launch with no re-link (`sign_in::check_launch`).
pub(super) async fn link_reused_after(
    clock: Arc<dyn crate::clock::Clock>,
    matter: clauseo_protocol::MatterId,
    codex: &mut CodexPrepared,
) {
    // Linked or left out already (a key login).
    if codex.link.is_some() {
        return;
    }
    let holds = codex
        .login
        .is_none_or(|checked| checked.link == Seen::Answered(LoginLink::Shared));
    if !holds {
        return;
    }
    let (home, state) = (codex.home.clone(), codex.state.clone());
    let job = codex::prepare::Job::short("login link", Some(matter));
    let linked = codex::prepare::off_actor(clock, job, move || {
        state_folder::link_reused(&home, &state).map_err(|error| error.refusal())
    })
    .await;
    codex.link = Some(linked);
}

/// What a launch does with its matter's state folder, read on the actor.
#[derive(Debug, Clone, Copy)]
pub(super) struct StateUse {
    /// Another session of the agent runs in the matter: no clean
    /// (`state_folder::prepare_in_use`), which would remove that session's files.
    pub(super) in_use: bool,
    /// The matter's Codex login link is stale (it outlived a login or source change while a
    /// session ran there, `turn/sign_in.rs`) and no other session runs there now: the launch
    /// removes it before it links. The mark is taken.
    pub(super) drop_stale: bool,
    /// The launch's environment will carry `CODEX_API_KEY` (desktop's key login, through the
    /// hook's env; or the settings' or inherited value): no `auth.json` is linked, and one
    /// there is removed, so Codex can use only the key and no shared login file is in reach.
    pub(super) key_login: bool,
}

/// The variable a Codex key login reaches the agent in.
const KEY_VAR: &str = "CODEX_API_KEY";

/// Whether `thread`'s Codex launch environment will carry a non-empty [`KEY_VAR`], read in the
/// launch spec's order (the hook's env over the settings' agent variables over the inherited
/// environment, `agent::launch_spec`).
fn key_login(actor: &Actor, agent: &clauseo_protocol::AgentId) -> bool {
    if agent.as_str() != AgentKind::Codex.id() {
        return false;
    }
    let hook = actor.profile.launch_env(agent);
    if let Some((_, value)) = hook.iter().find(|(name, _)| name.as_os_str() == KEY_VAR) {
        return !value.is_empty();
    }
    let settings = actor.effective.agent(agent.as_str());
    if let Some((_, value)) = settings
        .iter()
        .flat_map(|launch| launch.env.iter())
        .find(|(name, _)| name.as_str() == KEY_VAR)
    {
        return !value.is_empty();
    }
    match &actor.wiring.inherited_env {
        Some(env) => env
            .iter()
            .any(|(name, value)| name.as_os_str() == KEY_VAR && !value.is_empty()),
        None => std::env::var_os(KEY_VAR).is_some_and(|value| !value.is_empty()),
    }
}

/// [`StateUse`] for `thread`'s launch. A matter whose link is stale while a session from before
/// the change still runs there refuses the launch with `agent.launch_failed` (retryable): that
/// session holds the old login under the name `auth.json`, so a re-link would let its token
/// refresh write the old login's tokens into the new login's file. Not a split: reuse goes on.
/// Once that session ends, the next launch removes the stale link and links as usual.
pub(super) fn state_use(actor: &mut Actor, thread: ThreadId) -> Result<StateUse, Error> {
    let Some((agent, matter)) = actor
        .threads
        .get(&thread)
        .map(|entry| (entry.row.agent.clone(), entry.row.matter))
    else {
        return Ok(StateUse {
            in_use: false,
            drop_stale: false,
            key_login: false,
        });
    };
    let in_use =
        super::super::sign_in::running_matters(actor, &agent, Some(thread)).contains(&matter);
    // Only Codex links its login; the marks are Codex's.
    let stale = agent.as_str() == AgentKind::Codex.id()
        && actor.wiring.sign_in.stale_links.contains(&matter);
    if stale && in_use {
        tracing::warn!(
            %agent,
            "the matter's login link is in use by a session from before the login changed; \
             the launch is refused"
        );
        return Err(Error::refused(
            clauseo_protocol::ErrorCode::AgentLaunchFailed,
            "Codex can't start here until this matter's other Codex session, started before the \
             sign-in changed, has ended.",
        ));
    }
    let drop_stale = stale && actor.wiring.sign_in.stale_links.remove(&matter);
    let key_login = key_login(actor, &agent);
    Ok(StateUse {
        in_use,
        drop_stale,
        key_login,
    })
}

/// Whether desktop has said `thread`'s agent's login is reused (`HostHandle::set_reused_login`).
pub(super) fn reused_login(actor: &Actor, thread: ThreadId) -> bool {
    actor
        .threads
        .get(&thread)
        .is_some_and(|entry| actor.wiring.sign_in.reused.contains(&entry.row.agent))
}
