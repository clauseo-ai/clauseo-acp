//! The matter's state folder in a launch (`agent/state_folder.rs`, R17 PR-C), on the launch-prep
//! thread: Codex's and Claude's preparation with it, and the reused Codex login's link, made once
//! the sign-in hook has answered.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::{AgentKind, ThreadId};

use super::{CodexPrepared, Prepared, RunFolders};
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
    codex::write_home_config(&state)?;
    // The stale link's removal and the launch's own link run under the folder's lock, so a
    // login change's removal (`login_link::remove_links_but`) never interleaves with them.
    let lock = state_folder::folder_lock(&state);
    let held = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state_use.drop_stale {
        state_folder::remove_stale_link(&state).map_err(|error| {
            tracing::warn!(error_kind = ?error.kind(), "a stale login link was not removed");
            Error::refused(
                clauseo_protocol::ErrorCode::AgentLaunchFailed,
                "Clauseo couldn't prepare Codex's folder.",
            )
        })?;
    }
    // A key login gets no login file; Clauseo's own login is linked now; a reused one once the
    // hook has answered (`link_reused_after`).
    let link = if state_use.key_login {
        Some(unlink_for_key(&state, state_use.in_use))
    } else {
        (!reused).then(|| state_folder::link_own(&home, &state).map_err(|error| error.refusal()))
    };
    drop(held);
    // The `agent-state` denies of the shell's fence, read here off the actor.
    let state_denies = crate::agent::codex_fence::state_denies(data, &state);
    let prepared = CodexPrepared {
        folder,
        home,
        state,
        keys: serde_json::Map::new(),
        login: None,
        link,
        lawyers_login: false,
        state_denies,
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
        let lock = state_folder::folder_lock(&state);
        let _held = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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

/// What a launch takes from the actor for the state folder: whether the agent's login is
/// reused, the matter's [`StateUse`], and, when no launch gate is wired
/// (`HostConfig::with_launch_gate`, #554), the agent, whose launch with a state folder is
/// refused.
pub(super) struct StatePlan {
    /// The agent's login is reused (`reused_login`).
    pub(super) reused: bool,
    /// The launch goes through the sandbox's launchers: a reused Codex login is never linked
    /// into its state folder, and the launch is refused (`sign_in::check_launch`).
    jailed: bool,
    /// For a jailed Codex launch, the lawyer's own login files (`lawyers_logins`), compared by
    /// identity with the state folder's `auth.json`; none otherwise.
    lawyers: Vec<PathBuf>,
    state_use: StateUse,
    /// A launch gate is wired.
    gated: bool,
    /// The thread's agent, for the refusal's log.
    agent: Option<clauseo_protocol::AgentId>,
}

/// An agent's own preparation ([`StatePlan::prepare_agent`]): Codex's preparation, Claude's
/// home and the folder check's answer.
pub(super) type AgentPrep = (Option<CodexPrepared>, Option<PathBuf>, Option<Answer>);

impl StatePlan {
    /// The plan for `thread`'s launch, read on the actor; an error fails the launch.
    pub(super) fn of(actor: &mut Actor, thread: ThreadId) -> Result<Self, Error> {
        let reused = reused_login(actor, thread);
        let gated = actor.wiring.launch_gate.is_some();
        let agent = actor
            .threads
            .get(&thread)
            .map(|entry| entry.row.agent.clone());
        let state_use = state_use(actor, thread)?;
        let jailed = actor.protection.launchers().is_some();
        let codex = agent
            .as_ref()
            .is_some_and(|agent| agent.as_str() == AgentKind::Codex.id());
        let lawyers = if jailed && codex {
            lawyers_logins(actor)
        } else {
            Vec::new()
        };
        Ok(Self {
            reused,
            jailed,
            lawyers,
            state_use,
            gated,
            agent,
        })
    }

    /// Whether the reused login's link is made once the sign-in hook has answered
    /// ([`link_reused`]): never for a jailed launch, which the jail never gives the lawyer's
    /// own `auth.json` (orchestrator's ruling (a)).
    pub(super) const fn links_reused(&self) -> bool {
        self.reused && !self.jailed
    }

    /// The agent's own preparation on the launch-prep thread, after the data-folder refusals:
    /// a launch with a state folder refused when no launch gate is wired (fail closed: it shares
    /// the one login), then Codex's (`home` its login home, [`prepare_codex`]) or Claude's
    /// ([`prepare_claude`]). Returns Codex's preparation, Claude's home and the folder check's
    /// answer. Claude is never refused for its folder: the answer decides its forced ask.
    pub(super) fn prepare_agent(
        &self,
        check: &FolderCheck,
        stored: &Path,
        data: &Path,
        home: Option<PathBuf>,
        claude: bool,
        run: &RunFolders,
    ) -> Result<AgentPrep, Error> {
        if run.state.is_some() && !self.gated {
            refuse_ungated(self.agent.as_ref())?;
        }
        let mut answer = None;
        let (codex_home, claude_home) = if claude { (None, home) } else { (home, None) };
        let codex = match (codex_home, run.state.clone()) {
            (Some(home), Some(state)) => {
                let (mut prepared, support) = prepare_codex(
                    check,
                    stored,
                    data,
                    home,
                    state,
                    self.reused,
                    self.state_use,
                )?;
                prepared.lawyers_login = is_lawyers_login(&prepared, &self.lawyers);
                if prepared.lawyers_login && !self.state_use.in_use {
                    drop_lawyers_link(&prepared.state);
                }
                answer = Some(support);
                Some(prepared)
            }
            _ => None,
        };
        if let Some(home) = &claude_home {
            answer = Some(check.run(&crate::agent::session_folder(stored)));
            prepare_claude(data, home, run.state.as_deref(), self.state_use.in_use)?;
        }
        Ok((codex, claude_home, answer))
    }
}

/// The lawyer's own Codex login files, as paths: `~/.codex/auth.json` under the lawyer's home,
/// and `$CODEX_HOME/auth.json` when the lawyer's environment (the inherited one, as a launch
/// reads it) sets an absolute `CODEX_HOME`. Read on the actor, no file system call.
fn lawyers_logins(actor: &Actor) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = actor
        .home
        .iter()
        .map(|home| home.join(".codex").join(state_folder::LOGIN_FILE))
        .collect();
    let codex_home = match &actor.wiring.inherited_env {
        Some(env) => env
            .iter()
            .find(|(name, _)| name.as_os_str() == codex::HOME_VAR)
            .map(|(_, value)| value.clone()),
        None => std::env::var_os(codex::HOME_VAR),
    };
    files.extend(
        codex_home
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
            .map(|home| home.join(state_folder::LOGIN_FILE)),
    );
    files
}

/// Whether `codex`'s state folder `auth.json`, linked (`Same` or `Linked`), is one of the
/// lawyer's own login files `lawyers` by file identity (device and inode, volume and file id),
/// wherever its name is: Clauseo's login home may hold a name of the lawyer's file. Never by
/// link count, which Clauseo's own login legitimately raises (one name per matter). File system
/// calls: off the actor.
pub(super) fn is_lawyers_login(codex: &CodexPrepared, lawyers: &[PathBuf]) -> bool {
    use state_folder::StateLink;
    let linked = matches!(codex.link, Some(Ok(StateLink::Same | StateLink::Linked)));
    let file = codex.state.join(state_folder::LOGIN_FILE);
    linked
        && lawyers
            .iter()
            .any(|lawyers| state_folder::same_file(&file, lawyers).unwrap_or(false))
}

/// A jailed launch's state folder `auth.json` that is the lawyer's own file: its name in the
/// state folder is removed, under the folder's lock (never the file it names, never anything of
/// the lawyer's `~/.codex`), so no jail of the matter finds it; the launch is refused
/// (`sign_in::check_launch`). A failure is logged by its kind and changes nothing else.
fn drop_lawyers_link(state: &Path) {
    let lock = state_folder::folder_lock(state);
    let _held = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Err(error) = state_folder::remove_stale_link(state) {
        tracing::warn!(error_kind = ?error.kind(), "the lawyer's login link was not removed");
    }
}

/// The refusal of a launch with a state folder when no launch gate is wired: `agent.launch_failed`,
/// logged with the agent.
fn refuse_ungated(agent: Option<&clauseo_protocol::AgentId>) -> Result<(), Error> {
    let agent = agent
        .map(clauseo_protocol::AgentId::as_str)
        .unwrap_or_default();
    tracing::warn!(agent, "no identity check is wired; the launch is refused");
    Err(Error::refused(
        clauseo_protocol::ErrorCode::AgentLaunchFailed,
        "Clauseo can't start the agent: no sign-in check is set up.",
    ))
}

/// A prepared launch on a `reused` login, once the sign-in hook has answered: the reused
/// login's link made in the matter's state folder ([`link_reused_after`]).
pub(super) async fn link_reused(
    result: Result<Prepared, Error>,
    reused: bool,
    clock: Arc<dyn crate::clock::Clock>,
    matter: clauseo_protocol::MatterId,
) -> Result<Prepared, Error> {
    match result {
        Ok(mut prepared) if reused => {
            if let Some(codex) = prepared.codex.as_mut() {
                link_reused_after(clock, matter, codex).await;
            }
            Ok(prepared)
        }
        result => result,
    }
}
