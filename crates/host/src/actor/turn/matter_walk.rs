//! The matter's link walk at each turn's start (`data_fence/matter_links.rs`). A running
//! agent whose launch gave it the matter's read allow rules (`Agent::matter_rules`) keeps them
//! for the whole session: Claude Code 2.1.293 reads its `--settings` file once, at its start,
//! and pins what it read (`agent/launch_settings.rs`). So a link out of the matter that appears
//! after the launch with no judged command (a zip unpacked with its links, a sync client) would
//! let a glob through it read `<data>`, another matter or `~/.ssh` with no classifier.
//!
//! So at step 4 of each turn on such an agent, before its checkpoint, the walk runs again, off
//! the actor and within `WALK_BUDGET` (`data_fence::readable_matter_within`); the turn stays
//! `Checkpointing` meanwhile, so a cancel ends it as it would during the checkpoint. A walk that
//! finds a link out, or can't finish in time, stops the agent and starts a new one for the turn
//! (`agents::restart_for_links`, as for a moved matter): the new launch walks again, writes no
//! matter rules, and resumes the session with `session/load`. A walk that finds none goes on to
//! the checkpoint. The turn an agent was launched for is not walked again: its launch just did.
//! Nor is an agent-started turn, whose agent is already at work. The residual is a link made
//! during one turn, after its walk.

use std::sync::Arc;

use clauseo_protocol::{ThreadId, TurnId};

use super::super::Actor;
use super::super::input::Input;
use super::agents::{self, Stage, Waiting};
use super::{TurnInput, TurnPhase, current, matter_of, state, steps};
use crate::agent::data_fence;

/// An agent with the matter's read rules: the last turn whose start walked the matter's links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Walks {
    /// The turn it was launched for, whose launch walked them, or the last one walked here.
    pub(super) walked_for: Option<TurnId>,
}

impl Walks {
    /// An agent launched for `waiting`: its launch walked for the turn among them.
    pub(super) fn launched_for(waiting: &[Waiting]) -> Self {
        let walked_for = waiting.iter().find_map(|waiting| match waiting {
            Waiting::Turn(turn) => Some(*turn),
            Waiting::Config(..) | Waiting::Mode(..) => None,
        });
        Self { walked_for }
    }
}

/// A turn's walk answered.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Walked {
    thread: ThreadId,
    turn: TurnId,
    /// The agent whose rules were walked.
    agent: u64,
    /// The matter still may get its read rules: no link leads out, and the walk finished.
    keep: bool,
}

/// Starts `turn`'s walk when its agent has the matter's rules and was not launched for it:
/// `true` when the turn now waits for [`on_walked`].
pub(super) fn start(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> bool {
    if super::own_turn::is_own(actor, thread) {
        return false;
    }
    let Ok(matter) = matter_of(actor, thread) else {
        return false;
    };
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return false;
    };
    if agent.stage != Stage::Attached {
        return false;
    }
    let Some(walks) = agent.matter_rules.as_mut() else {
        return false;
    };
    if walks.walked_for.replace(turn) == Some(turn) {
        return false;
    }
    let (id, stored) = (agent.id, agent.folder.clone());
    let clock = Arc::clone(&actor.clock);
    actor.spawn_post(async move {
        let readable = data_fence::readable_matter_within(clock, matter, stored).await;
        Input::Turn(TurnInput::MatterWalked(Walked {
            thread,
            turn,
            agent: id,
            keep: !readable.is_empty(),
        }))
    });
    true
}

/// The walk answered: a matter that lost its rules starts the agent again for the turn, unless
/// the turn was cancelled meanwhile or the agent is gone; otherwise the checkpoint goes on.
pub(super) fn on_walked(actor: &mut Actor, walked: Walked) {
    let Walked {
        thread,
        turn,
        agent,
        keep,
    } = walked;
    let Some(found) = current(actor, thread)
        .filter(|current| current.turn == turn && current.phase == TurnPhase::Checkpointing)
    else {
        return;
    };
    let cancelled = found.cancelled;
    if !keep && !cancelled && agents::restart_for_links(actor, thread, turn, agent) {
        return;
    }
    steps::checkpoint(actor, thread, turn);
}
