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
/// `config.toml`, and on Clauseo's own login (not `reused`) its `auth.json` linked.
pub(super) fn prepare_codex(
    check: &FolderCheck,
    stored: &Path,
    data: &Path,
    home: PathBuf,
    state: PathBuf,
    reused: bool,
) -> Result<(CodexPrepared, Answer), Error> {
    let (folder, support) = codex::prepare_launch(check, stored, data, &home)?;
    state_folder::prepare(data, &state, AgentKind::Codex)?;
    codex::write_home_config(&state)?;
    // Clauseo's own login is linked now; a reused one once the hook has answered
    // (`link_reused_after`).
    let link =
        (!reused).then(|| state_folder::link_own(&home, &state).map_err(|error| error.refusal()));
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

/// A Claude launch's folders on the launch-prep thread: its login home `home` made, with
/// Claude Code's user settings (a failure is a warning), then the matter's state folder
/// `state` made, cleaned and given its `settings.json` (a failure fails the launch).
pub(super) fn prepare_claude(data: &Path, home: &Path, state: Option<&Path>) -> Result<(), Error> {
    codex::make_agent_home(data, home, "Claude Code")?;
    if let Err(error) = crate::agent::home::write_claude_settings(home) {
        tracing::warn!(error = %Cause(&error), "Claude's home settings were not written");
    }
    let Some(state) = state else {
        return Ok(());
    };
    state_folder::prepare(data, state, AgentKind::Claude)?;
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

/// Whether desktop has said `thread`'s agent's login is reused (`HostHandle::set_reused_login`).
pub(super) fn reused_login(actor: &Actor, thread: ThreadId) -> bool {
    actor
        .threads
        .get(&thread)
        .is_some_and(|entry| actor.wiring.sign_in.reused.contains(&entry.row.agent))
}
