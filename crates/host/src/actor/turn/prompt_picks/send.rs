//! Setting a turn's own picks at step 4, before its checkpoint and `TurnStarted` (module doc of
//! `prompt_picks.rs`).
//!
//! - The model pick goes first, when the thread's options offer it and do not hold it. The
//!   effort pick is then checked against the options of the model's answer (else the thread's,
//!   when no model was set): sent when that answer offers it and does not hold it already. A
//!   Claude model switch gives the new model its own default effort (claude-agent-acp 0.110.1,
//!   `effortForModelSwitch`, `session-effort.js:152-169`), so an effort equal to the old one is
//!   still sent, and one only the new model offers is not dropped.
//! - A model or effort pick the options do not offer, one the agent refuses, and every pick
//!   when [`PICKS_WAIT`] runs out before the agent's answers, could not be used: the turn's
//!   `TurnStarted` is followed by one `NoticeKind::PickNotUsed`. So are picks to set when no
//!   session is attached at step 4, and picks whose answer came from an agent since replaced.
//! - Past [`PICKS_WAIT`] the turn goes on with the thread's options, but the sets are not
//!   dropped: they finish in the background, and their answer, when it comes, is logged as
//!   `ConfigOptionsReplaced`, so the thread's options say what the session holds and a later
//!   pick is compared against that. A pick of an option the session does not report at all is
//!   not known to be a model or effort one, and goes without a word, as before.
//! - The answer always comes back to the actor ([`on_picked`]), whatever happened, and the
//!   checkpoint then runs: a cancel or an agent's end meanwhile is handled there as for any
//!   checkpointing turn.

use std::time::Duration;

use clauseo_protocol::{
    AgentChoiceId, AgentSessionId, ConfigCategory, ConfigOption, ConfigOptionsReplaced,
    ConfigValue, NamedChoice, Notice, NoticeKind, NoticeLevel, ThreadId, TurnId,
};

use super::super::super::Actor;
use super::super::agents::Stage;
use super::super::out::Out;
use super::super::picks::Pick;
use super::super::{AgentReply, Then, TurnInput, TurnPhase, current, state, steps};
use super::{Ran, select};
use crate::actor::input::Input;
use crate::log_file::{Cause, LIFECYCLE};

/// The longest the picks may hold a turn's start: past it, those unsent could not be used.
pub(super) const PICKS_WAIT: Duration = Duration::from_secs(20);

/// What setting a turn's picks did, for [`on_picked`].
#[derive(Debug, Default)]
pub(crate) struct Picked {
    /// The agent's last answer, when it took a set.
    answer: Option<Vec<ConfigOption>>,
    /// A pick could not be used.
    not_used: bool,
    /// Not an answer: [`PICKS_WAIT`] ran out first.
    timed_out: bool,
}

/// What to set: the model pick and the effort pick still to check; `not_used` when a pick is
/// already known not to be usable.
#[derive(Debug, Default)]
struct Sets {
    model: Option<Pick>,
    effort: Option<Pick>,
    not_used: bool,
}

/// How an effort pick stands against the options the session holds now.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Effort {
    /// Offered and not held: it is sent.
    Send,
    /// Already held: nothing to send.
    Held,
    /// An effort option that does not offer it: it could not be used.
    NotOffered,
    /// No effort option of that id: not known to be an effort pick.
    Unknown,
}

/// Step 4's picks for `turn`: sends them, off the actor, when the attached session has any to
/// take, and answers true (the checkpoint then waits for [`on_picked`]). With none to send it
/// notes what could not be used, for the turn's start, and answers false.
pub(in crate::actor::turn) fn send_first(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
) -> bool {
    let Some(picks) = state(actor, thread).and_then(|state| {
        let picks = &state.prompt_picks;
        let fresh = !picks.ran.contains_key(&turn);
        picks.by_turn.get(&turn).filter(|_| fresh).cloned()
    }) else {
        return false;
    };
    let options = super::super::picks::thread_options(actor, thread);
    let sets = sets(&options, picks.as_map());
    let agent = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .filter(|agent| agent.stage == Stage::Attached)
        .and_then(|agent| Some((agent.id, agent.link.client.clone(), agent.session.clone()?)));
    let to_set = sets.model.is_some() || sets.effort.is_some();
    let attached = agent.is_some();
    let (Some((id, client, session)), true) = (agent, to_set) else {
        // Picks to set and no attached session to take them: they could not be used.
        let unsent = to_set && !attached;
        if let Some(state) = state(actor, thread) {
            let not_used = sets.not_used || unsent;
            state.prompt_picks.ran.insert(
                turn,
                Ran {
                    not_used,
                    ..Ran::default()
                },
            );
        }
        return false;
    };
    // The first of the answer and the wait's end goes on with the turn (`on_picked`).
    let timed_out = Picked {
        not_used: true,
        timed_out: true,
        ..Picked::default()
    };
    let wait = actor.post_after(
        PICKS_WAIT,
        Input::Turn(TurnInput::Agent {
            thread,
            agent: id,
            reply: AgentReply::PromptPicked {
                turn,
                picked: timed_out,
            },
        }),
    );
    if let Some(state) = state(actor, thread) {
        let ran = Ran {
            waiting: Some(wait),
            ..Ran::default()
        };
        state.prompt_picks.ran.insert(turn, ran);
    }
    super::super::agents::call(actor, thread, id, async move {
        let picked = send(&client, &session, (options, sets)).await;
        AgentReply::PromptPicked { turn, picked }
    });
    true
}

/// The picks of `wanted` (a turn's) that matter at step 4: the model pick when `options` offer
/// it and do not hold it, the effort pick to check after it. A pick of a reported effort option
/// is never replaced by one of an unreported option or of one that is not a select.
fn sets(
    options: &[ConfigOption],
    wanted: &std::collections::BTreeMap<AgentChoiceId, AgentChoiceId>,
) -> Sets {
    let mut sets = Sets::default();
    for (option, value) in wanted {
        let pick = (option.clone(), value.clone());
        match select(options, option.as_str()) {
            Some((ConfigCategory::Model, current, offered)) => {
                if !offered.iter().any(|(id, _)| id == value.as_str()) {
                    sets.not_used = true;
                } else if current != value.as_str() {
                    sets.model = Some(pick);
                }
            }
            // An effort option: checked against the model's answer.
            Some((ConfigCategory::ThoughtLevel, ..)) => sets.effort = Some(pick),
            // A reported option that is not a select (Claude's fast mode): no model or effort.
            None if options.iter().any(|known| known.id == option.as_str()) => {}
            // One the session does not report yet (a model switch may bring it): checked as an
            // effort pick, unless a reported effort option is picked.
            None => {
                if sets.effort.is_none() {
                    sets.effort = Some(pick);
                }
            }
            Some(_) => {}
        }
    }
    sets
}

/// How the effort pick `(option, value)` stands against `options`.
pub(super) fn effort(options: &[ConfigOption], (option, value): &Pick) -> Effort {
    match select(options, option.as_str()) {
        Some((ConfigCategory::ThoughtLevel, current, offered)) => {
            if current == value.as_str() {
                Effort::Held
            } else if offered.iter().any(|(id, _)| id == value.as_str()) {
                Effort::Send
            } else {
                Effort::NotOffered
            }
        }
        Some(_) | None => Effort::Unknown,
    }
}

/// Sets `sets` on `session`, the model first, then the effort as the model's answer stands.
/// Nothing bounds it here: [`PICKS_WAIT`] bounds the turn's wait for it, not the sets.
async fn send(
    client: &clauseo_acp::AcpClient,
    session: &AgentSessionId,
    (options, sets): (Vec<ConfigOption>, Sets),
) -> Picked {
    let mut picked = Picked {
        not_used: sets.not_used,
        ..Picked::default()
    };
    let mut latest = options;
    if let Some(pick) = sets.model {
        match set(client, session, &pick).await {
            Some(answer) => {
                latest.clone_from(&answer);
                picked.answer = Some(answer);
            }
            None => picked.not_used = true,
        }
    }
    let Some(pick) = sets.effort else {
        return picked;
    };
    match effort(&latest, &pick) {
        Effort::Send => match set(client, session, &pick).await {
            Some(answer) => picked.answer = Some(answer),
            None => picked.not_used = true,
        },
        Effort::NotOffered => picked.not_used = true,
        Effort::Held | Effort::Unknown => {}
    }
    picked
}

/// One `session/set_config_option`; the agent's answer, or `None` when it refused.
async fn set(
    client: &clauseo_acp::AcpClient,
    session: &AgentSessionId,
    (option, value): &Pick,
) -> Option<Vec<ConfigOption>> {
    let select = ConfigValue::Select(value.clone());
    match client
        .set_config_option(session, option.clone(), select)
        .await
    {
        Ok(options) => {
            tracing::info!(target: LIFECYCLE, %option, %value, "a prompt's pick was set");
            Some(options)
        }
        Err(error) => {
            tracing::info!(%option, %value, error = %Cause(&error), "a prompt's pick was not set");
            None
        }
    }
}

/// `turn`'s picks were set, from agent `same` or not, or [`PICKS_WAIT`] ran out first: the
/// first of the two is kept for its `TurnStarted` and runs its checkpoint. The agent's last
/// answer is logged whenever it comes, after the wait's end too.
pub(in crate::actor::turn) fn on_picked(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    same: bool,
    picked: Picked,
) {
    let waiting = state(actor, thread)
        .and_then(|state| state.prompt_picks.ran.get_mut(&turn))
        .and_then(|ran| ran.waiting.take());
    if picked.timed_out {
        if waiting.is_some() {
            tracing::warn!(%thread, "a prompt's picks were not all set in time; the turn goes on");
            went_on(actor, thread, turn, Ran::from(picked));
        }
        return;
    }
    if let Some(wait) = &waiting {
        wait.abort();
    }
    let answer = picked.answer.filter(|_| same);
    if let Some(options) = &answer {
        let options = options.clone();
        let logged = Out::new(actor, thread, None).and_then(|mut out| {
            out.push(actor, ConfigOptionsReplaced { options })?;
            out.send(actor, Then::Nothing)
        });
        if let Err(error) = logged {
            tracing::warn!(%thread, error = %Cause(&error), "a prompt's picks were not logged");
        }
    }
    if waiting.is_none() {
        // After the wait's end: the turn went on without it.
        return;
    }
    // An answer from an agent since replaced says nothing of the session the turn runs on.
    let not_used = picked.not_used || !same;
    let ran = Ran {
        answer,
        not_used,
        waiting: None,
    };
    went_on(actor, thread, turn, ran);
}

/// `turn` goes on with `ran`, what its picks did: kept for its `TurnStarted`, and its
/// checkpoint runs when it waits for them.
fn went_on(actor: &mut Actor, thread: ThreadId, turn: TurnId, ran: Ran) {
    if let Some(state) = state(actor, thread) {
        state.prompt_picks.ran.insert(turn, ran);
    }
    let waiting = current(actor, thread)
        .is_some_and(|current| current.turn == turn && current.phase == TurnPhase::Checkpointing);
    if waiting {
        steps::checkpoint(actor, thread, turn);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
impl Picked {
    /// The agent's answer `options`, every pick taken.
    pub(in crate::actor::turn) fn answered(options: Vec<ConfigOption>) -> Self {
        Self {
            answer: Some(options),
            ..Self::default()
        }
    }
}

/// `turn`'s sets are out, as [`send_first`] leaves them, with no wait armed.
#[cfg(test)]
pub(in crate::actor::turn) fn sets_out(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let (wait, _registration) = futures::future::AbortHandle::new_pair();
    if let Some(state) = state(actor, thread) {
        let ran = Ran {
            waiting: Some(wait),
            ..Ran::default()
        };
        state.prompt_picks.ran.insert(turn, ran);
    }
}

impl From<Picked> for Ran {
    fn from(picked: Picked) -> Self {
        Self {
            answer: picked.answer,
            not_used: picked.not_used,
            waiting: None,
        }
    }
}

/// The notice for a pick that could not be used, naming `model` and `effort`, what the turn
/// runs with.
pub(super) fn not_used(model: Option<&NamedChoice>, effort: Option<&NamedChoice>) -> Notice {
    let model = model.map(|model| model.name.to_string());
    let effort = effort.map(|effort| effort.name.to_string());
    Notice {
        level: NoticeLevel::Info,
        text: not_used_text(model.as_deref(), effort.as_deref()),
        kind: Some(NoticeKind::PickNotUsed { model, effort }),
    }
}

/// The host English of [`not_used`], for a view that does not know the kind.
pub(crate) fn not_used_text(model: Option<&str>, effort: Option<&str>) -> String {
    let held = match (model, effort) {
        (Some(model), Some(effort)) => format!("{model} at {effort} effort"),
        (Some(model), None) => model.to_owned(),
        (None, Some(effort)) => format!("{effort} effort"),
        (None, None) => "the settings the conversation already had".to_owned(),
    };
    format!(
        "Clauseo couldn't switch to the model or effort you picked for this answer, so it used \
         {held}."
    )
}
