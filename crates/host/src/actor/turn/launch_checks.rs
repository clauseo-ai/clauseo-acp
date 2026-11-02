//! An agent's launch, in two phases so no file system call runs on the actor. Phase one sends
//! the checks to a `clauseo-codex-fs` thread (`agent/codex/prepare.rs`): for every agent the
//! data-folder refusal (`agent/data_folder.rs`), and for a Codex agent its preparation
//! ([`crate::agent::codex::prepare_launch`]: the matter folder resolved through links, the
//! `.codex` refusal, the home made and the home's `config.toml`). The work that waits for the
//! agent is held on the thread's turn state meanwhile. Phase two, when they answer, launches
//! the agent on the actor with what they found ([`agents::launch_now`]), or fails the waiting
//! work.

use std::path::PathBuf;

use clauseo_protocol::{AgentKind, ThreadId};

use super::super::Actor;
use super::super::input::Input;
use super::agents::{self, Waiting};
use super::{TurnInput, host_stopping, state};
use crate::agent::{codex, data_folder};
use crate::error::Error;

/// A launch whose checks are running.
#[derive(Debug)]
pub(super) struct Preparing {
    /// Its id: an answer for an earlier launch's checks is dropped.
    id: u64,
    /// The work that waits for the agent's session.
    pub(super) waiting: Vec<Waiting>,
}

/// What a launch's checks found.
#[derive(Debug)]
pub(crate) struct Prepared {
    /// The matter folder as the thread's matter row stored it: the agent's working directory.
    pub(super) stored: PathBuf,
    /// A Codex agent's preparation; `None` for any other agent.
    pub(super) codex: Option<CodexPrepared>,
}

/// What a Codex launch's preparation found.
#[derive(Debug)]
pub(crate) struct CodexPrepared {
    /// The matter folder resolved through links: `_codex/hooks/list` names it.
    pub(super) folder: PathBuf,
    /// The agent's home, made and checked, as `CODEX_HOME`.
    pub(super) home: PathBuf,
}

/// Phase one: the checks go to their thread, and `waiting` waits on the thread's state.
pub(super) fn prepare(actor: &mut Actor, thread: ThreadId, waiting: Vec<Waiting>) {
    let paths = (|| -> Result<_, Error> {
        let row = &actor.check_thread(thread)?.row;
        let stored = PathBuf::from(&actor.check_matter(row.matter)?.row.folder_path);
        let locations = actor.effective.locations();
        let codex = AgentKind::of(&row.agent) == Some(AgentKind::Codex);
        let home = codex.then(|| locations.agent_home(row.agent.as_str()));
        Ok((stored, locations.data_dir().to_owned(), home))
    })();
    let (stored, data, home) = match paths {
        Ok(paths) => paths,
        Err(error) => {
            agents::fail_all(actor, thread, waiting, &error);
            return;
        }
    };
    let id = agents::next_agent_id();
    let Some(state) = state(actor, thread) else {
        return;
    };
    state.preparing = Some(Box::new(Preparing { id, waiting }));
    let clock = std::sync::Arc::clone(&actor.clock);
    actor.spawn_post(async move {
        let result = codex::prepare::off_actor(clock, move || {
            data_folder::refuse_data_folder(&stored, &data)?;
            let codex = match home {
                Some(home) => {
                    let folder = codex::prepare_launch(&stored, &data, &home)?;
                    Some(CodexPrepared { folder, home })
                }
                None => None,
            };
            Ok(Prepared { stored, codex })
        })
        .await;
        Input::Turn(TurnInput::Prepared { thread, id, result })
    });
}

/// Phase two: the checks `id` answered. The agent launches with what they found, or the
/// waiting work fails with their error; during shutdown it fails as closed.
pub(super) fn on_prepared(
    actor: &mut Actor,
    thread: ThreadId,
    id: u64,
    result: Result<Prepared, Error>,
) {
    let Some(state) = state(actor, thread) else {
        return;
    };
    let Some(preparing) = state.preparing.take_if(|preparing| preparing.id == id) else {
        return;
    };
    let waiting = preparing.waiting;
    match result {
        Ok(_) if host_stopping(actor) => agents::fail_all(actor, thread, waiting, &Error::Closed),
        Ok(prepared) => agents::launch_now(actor, thread, waiting, prepared),
        Err(error) => agents::fail_all(actor, thread, waiting, &error),
    }
}
