//! A Codex agent's launch, in two phases so no file system call runs on the actor. Phase one
//! sends the preparation ([`crate::agent::codex::prepare_launch`]: the matter folder resolved
//! through links, the `.codex` refusal, the home made, the home overlap refusal and the home's
//! `config.toml`) to a `clauseo-codex-fs` thread, and holds the work that waits for the agent
//! on the thread's turn state meanwhile. Phase two, when it answers, launches the agent on the
//! actor with what it found ([`agents::launch_now`]), or fails the waiting work. Any other
//! agent launches at once, with no hop.

use std::path::PathBuf;

use clauseo_protocol::{AgentKind, ThreadId};

use super::super::Actor;
use super::super::input::Input;
use super::agents::{self, Waiting};
use super::{TurnInput, host_stopping, state};
use crate::agent::codex;
use crate::error::Error;

/// A Codex launch whose preparation is running.
#[derive(Debug)]
pub(super) struct Preparing {
    /// Its id: an answer for an earlier preparation is dropped.
    id: u64,
    /// The work that waits for the agent's session.
    pub(super) waiting: Vec<Waiting>,
}

/// What a Codex launch's preparation found.
#[derive(Debug)]
pub(crate) struct Prepared {
    /// The matter folder as the thread's matter row stored it: the agent's working directory.
    pub(super) stored: PathBuf,
    /// The same folder resolved through links: the `.codex` re-checks and
    /// `_codex/hooks/list` use it.
    pub(super) folder: PathBuf,
    /// The agent's home, made and checked, as `CODEX_HOME`.
    pub(super) home: PathBuf,
}

/// Whether `thread`'s agent is a Codex agent.
pub(super) fn is_codex(actor: &Actor, thread: ThreadId) -> bool {
    actor
        .threads
        .get(&thread)
        .is_some_and(|entry| AgentKind::of(&entry.row.agent) == Some(AgentKind::Codex))
}

/// Phase one: the preparation goes to its thread, and `waiting` waits on the thread's state.
pub(super) fn prepare(actor: &mut Actor, thread: ThreadId, waiting: Vec<Waiting>) {
    let paths = (|| -> Result<_, Error> {
        let row = &actor.check_thread(thread)?.row;
        let stored = PathBuf::from(&actor.check_matter(row.matter)?.row.folder_path);
        let locations = actor.effective.locations();
        let home = locations.agent_home(row.agent.as_str());
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
    state.codex_start = Some(Box::new(Preparing { id, waiting }));
    let clock = std::sync::Arc::clone(&actor.clock);
    actor.spawn_post(async move {
        let result = codex::prepare::off_actor(clock, move || {
            let folder = codex::prepare_launch(&stored, &data, &home)?;
            Ok(Prepared {
                stored,
                folder,
                home,
            })
        })
        .await;
        Input::Turn(TurnInput::CodexPrepared { thread, id, result })
    });
}

/// Phase two: the preparation `id` answered. The agent launches with what it found, or the
/// waiting work fails with its error; during shutdown it fails as closed.
pub(super) fn on_prepared(
    actor: &mut Actor,
    thread: ThreadId,
    id: u64,
    result: Result<Prepared, Error>,
) {
    let Some(state) = state(actor, thread) else {
        return;
    };
    let Some(preparing) = state.codex_start.take_if(|preparing| preparing.id == id) else {
        return;
    };
    let waiting = preparing.waiting;
    match result {
        Ok(_) if host_stopping(actor) => agents::fail_all(actor, thread, waiting, &Error::Closed),
        Ok(prepared) => agents::launch_now(actor, thread, waiting, Some(prepared)),
        Err(error) => agents::fail_all(actor, thread, waiting, &error),
    }
}
