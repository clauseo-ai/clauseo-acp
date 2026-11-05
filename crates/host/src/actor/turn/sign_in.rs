//! Sign-in on the turn path (R20 brief, "The agent starts only after sign-in", "The expired
//! sign-in", "The split check" and "The refresh race"; decision record 0048).
//!
//! - A turn failing with `agent.auth_required`, or with an error the sign-in-again test accepts
//!   ([`code_of`]), is held for the sign-in: it goes back to its queue's head, held
//!   (`queue/sign_in.rs`).
//! - `restart_agents` stops each live agent of an id that is not in a turn, with the shutdown
//!   ladder's stop, so the next question launches with the new login, and resumes each queue
//!   held for that agent's sign-in, once.
//! - `stop_agents` cancels each turn of an id through the Cancel path (the lawyer's Stop, which
//!   also ends what the turn left running), marks it to go back held, drops a launch being
//!   prepared and stops the agent, so no process of it runs while the login link changes.
//! - `set_reused_login` keeps desktop's word on the login's source, which turns on the split
//!   check ([`check_launch`]).

use clauseo_protocol::{AgentId, ErrorCode, StopReason, ThreadId, TurnId};

use super::super::Actor;
use super::super::input::SignInRequest;
use super::close::TurnClose;
use super::launch_checks::Prepared;
use super::queue::{self, Requeue};
use super::{Current, agents, cancel, current, state};
use crate::agent::login_link::{LoginLink, LoginSplit, Seen};
use crate::error::Error;

/// One sign-in call from a handle.
pub(crate) fn on_request(actor: &mut Actor, request: SignInRequest) {
    match request {
        SignInRequest::Restart { agent, reply } => {
            restart(actor, &agent);
            let _gone = reply.send(Ok(()));
        }
        SignInRequest::Stop { agent, reply } => {
            stop(actor, &agent);
            let _gone = reply.send(Ok(()));
        }
        SignInRequest::Reused {
            agent,
            reused,
            reply,
        } => {
            tracing::info!(%agent, reused, "the login's source changed");
            let flags = &mut actor.wiring.sign_in.reused;
            if reused {
                flags.insert(agent);
            } else {
                flags.remove(&agent);
            }
            let _gone = reply.send(Ok(()));
        }
        SignInRequest::LinkHomes { agent, reply } => {
            // Stage C's R17 names each matter's state folder linked to the login here; until
            // then no matter has a link.
            tracing::info!(%agent, "the matters' login links: none before stage C");
            let _gone = reply.send(Ok(Vec::new()));
        }
    }
}

/// The threads whose agent is `agent`.
fn threads_of(actor: &Actor, agent: &AgentId) -> Vec<ThreadId> {
    actor
        .threads
        .iter()
        .filter(|(_, entry)| entry.row.agent == *agent)
        .map(|(thread, _)| *thread)
        .collect()
}

/// `restart_agents`: idle agents stop; queues held for the sign-in move on, and a turn they
/// start waits for its stopped agent's end, then launches a new one.
fn restart(actor: &mut Actor, agent: &AgentId) {
    tracing::info!(%agent, "the agents restart for a new login");
    for thread in threads_of(actor, agent) {
        let idle = state(actor, thread)
            .is_some_and(|state| state.current.is_none() && state.agent.is_some());
        if idle {
            agents::stop(actor, thread);
        }
        queue::resume_sign_in(actor, thread);
    }
}

/// `stop_agents`: every turn of `agent` cancelled and marked to go back held, every launch
/// being prepared dropped (its waiting work fails as closed), every agent stopped.
fn stop(actor: &mut Actor, agent: &AgentId) {
    tracing::info!(%agent, "the agents stop while the login changes");
    for thread in threads_of(actor, agent) {
        if let Some(turn) = current(actor, thread)
            .filter(|current| !current.cancelled)
            .map(|current| current.turn)
        {
            if let Some(state) = state(actor, thread) {
                state.stopped_for_sign_in = Some(turn);
            }
            cancel::cancel_current(actor, thread);
        }
        if let Some(preparing) = state(actor, thread).and_then(|state| state.preparing.take()) {
            // Its answer, when it comes, is stale: the run folder it made is removed then.
            agents::fail_all(actor, thread, preparing.waiting, &Error::Closed);
        }
        agents::stop(actor, thread);
    }
}

/// The code a turn failing with an error of `code` and `text` ends with:
/// `agent.auth_required` for an error the sign-in-again test accepts, else `code`.
pub(super) fn code_of(actor: &Actor, code: ErrorCode, text: &str) -> ErrorCode {
    if code != ErrorCode::AgentAuthRequired && actor.wiring.sign_in.sign_in_again(text) {
        return ErrorCode::AgentAuthRequired;
    }
    code
}

/// The turn `current` ends with `close`: what goes back to the queue for the sign-in, when it
/// failed with `agent.auth_required`, or `stop_agents` cancelled it (`stopped` names it) and
/// it did not end `EndTurn`.
pub(super) fn requeue_of(
    current: &mut Current,
    stopped: Option<TurnId>,
    close: &TurnClose,
) -> Option<Requeue> {
    let failed = close
        .failure
        .as_ref()
        .is_some_and(|(code, _)| *code == ErrorCode::AgentAuthRequired);
    let stopped = stopped == Some(close.turn) && close.stop != StopReason::EndTurn;
    (failed || stopped).then(|| Requeue {
        turn: close.turn,
        content: std::mem::take(&mut current.prompt),
    })
}

/// The split check, as a Codex launch's checks come back: the check's answer is kept for the
/// guard; while desktop says the login is reused, an answer but [`LoginLink::Shared`] refuses
/// the launch with `agent.auth_required` (so its turn is held), sets the reused flag to false
/// and sends the split; no answer refuses it with `agent.launch_failed` and changes nothing.
pub(super) fn check_launch(
    actor: &mut Actor,
    thread: ThreadId,
    prepared: &Prepared,
) -> Result<(), Error> {
    let Some(seen) = prepared.codex.as_ref().and_then(|codex| codex.login) else {
        return Ok(());
    };
    let Some(agent) = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone())
    else {
        return Ok(());
    };
    let sign_in = &mut actor.wiring.sign_in;
    sign_in.seen.insert(agent.clone(), seen);
    if !sign_in.reused.contains(&agent) {
        return Ok(());
    }
    match seen {
        Seen::Answered(LoginLink::Shared) => Ok(()),
        Seen::Answered(link) => {
            tracing::warn!(%agent, ?link, "the reused login split; the launch is refused");
            sign_in.reused.remove(&agent);
            sign_in.send_split(LoginSplit {
                agent,
                lawyers_file_gone: link == LoginLink::Gone,
            });
            Err(Error::refused(
                ErrorCode::AgentAuthRequired,
                "the ChatGPT login Clauseo was using is no longer the one on this computer",
            ))
        }
        Seen::Unknown => Err(Error::refused(
            ErrorCode::AgentLaunchFailed,
            "Clauseo couldn't check the ChatGPT login in time",
        )),
    }
}
