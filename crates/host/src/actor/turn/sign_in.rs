//! Sign-in on the turn path (R20 brief, "The agent starts only after sign-in", "The expired
//! sign-in", "The split check" and "The refresh race"; decision record 0048).
//!
//! - A turn failing with `agent.auth_required`, or with an error the sign-in-again test accepts
//!   ([`code_of`]), is held for the sign-in: it goes back to its queue's head, held
//!   (`queue/sign_in.rs`).
//! - `restart_agents` stops each live agent of an id that is not in a turn, with the shutdown
//!   ladder's stop, so the next question launches with the new login, and resumes each queue
//!   held for that agent's sign-in, once.
//! - `stop_agents` (`stop_agents_for` with [`StopFor::LoginSwitch`]) cancels each turn of an id
//!   through the Cancel path (the lawyer's Stop, which also ends what the turn left running),
//!   marks it to go back held, drops a launch being prepared and stops the agent, so no process
//!   of it runs while the login link changes. It answers once the agents have ended or its wait
//!   ran out (`sign_in/stops.rs`), and holds the id, in memory only, until `restart_agents`: no
//!   launch of it starts ([`launch_refusal`]) and a prompt for it queues held (`queue/hold.rs`).
//!   The hold's [`StopFor`] is the reason its queues show (`queue/held.rs`).
//! - `set_launch_refused` refuses an id's new launches with desktop's code, running sessions
//!   left alone ([`launch_refusal`]). A launch meeting `agent.keys_loading` waits for it to be
//!   lifted first, at most `KEYS_WAIT` ([`keys_wait`]).
//! - `set_reused_login` keeps desktop's word on the login's source, which turns on the split
//!   check ([`check_launch`]).

use clauseo_protocol::{AgentId, ErrorCode, StopReason, ThreadId, TurnId};

use super::super::Actor;
use super::super::input::SignInRequest;
use super::close::TurnClose;
use super::launch_checks::Prepared;
use super::queue::{self, Requeue};
use super::{Current, agents, cancel, current, state};
use crate::agent::launch_refusals::{KEYS_WAIT, WAITED_OUT};
use crate::agent::login_link::{LoginLink, LoginSplit, Seen, StopFor};
use crate::error::Error;

mod stops;

pub(super) use stops::{StopWaits, agent_ended, on_timer as stop_timer};

/// One sign-in call from a handle.
pub(crate) fn on_request(actor: &mut Actor, request: SignInRequest) {
    match request {
        SignInRequest::Restart { agent, reply } => {
            restart(actor, &agent);
            let _gone = reply.send(Ok(()));
        }
        SignInRequest::Stop { agent, why, reply } => {
            stop(actor, &agent, why);
            stops::wait(actor, agent, reply);
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
        SignInRequest::LaunchRefused { agent, code, reply } => {
            let _gone = reply.send(set_refused(actor, agent, code));
        }
        SignInRequest::LimitsLogin { agent, reply } => {
            super::super::limits::login_changed(actor, agent);
            let _gone = reply.send(Ok(()));
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
    actor.wiring.sign_in.held.remove(agent);
    for thread in threads_of(actor, agent) {
        let idle = state(actor, thread)
            .is_some_and(|state| state.current.is_none() && state.agent.is_some());
        if idle {
            agents::stop(actor, thread);
        }
        queue::resume_sign_in(actor, thread);
        // A head sent now that the hold kept back starts too.
        queue::move_on(actor, thread);
        // A queue still held shows why, `stop_agents`' hold gone.
        queue::sync_held(actor, thread);
    }
}

/// `stop_agents`: every turn of `agent` cancelled and marked to go back held, every launch
/// being prepared dropped (its waiting work fails as closed), every agent stopped. `why` is the
/// reason its held queues show, the latest stop's.
fn stop(actor: &mut Actor, agent: &AgentId, why: StopFor) {
    tracing::info!(%agent, ?why, "the agents stop while the login changes");
    actor.wiring.sign_in.held.insert(agent.clone(), why);
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
        // A held queue now waits for the login's switch.
        queue::sync_held(actor, thread);
    }
}

/// Whether `stop_agents` holds `thread`'s agent: nothing of it launches, and a prompt for it
/// queues held.
pub(super) fn holds(actor: &Actor, thread: ThreadId) -> bool {
    actor
        .threads
        .get(&thread)
        .is_some_and(|entry| actor.wiring.sign_in.held.contains_key(&entry.row.agent))
}

/// The reason `stop_agents` holds `thread`'s agent with, when it does (`queue/held.rs`).
pub(super) fn stop_for(actor: &Actor, thread: ThreadId) -> Option<StopFor> {
    let entry = actor.threads.get(&thread)?;
    actor.wiring.sign_in.held.get(&entry.row.agent).copied()
}

/// Why a launch of `thread`'s agent may not start: `stop_agents` holds it, so what waits fails
/// with `agent.auth_required` and a turn goes back held for the sign-in; or desktop refuses
/// its new launches (`set_launch_refused`), so a turn fails with desktop's code.
pub(super) fn launch_refusal(actor: &Actor, thread: ThreadId) -> Option<Error> {
    if holds(actor, thread) {
        return Some(Error::refused(
            ErrorCode::AgentAuthRequired,
            "the agent starts again once its sign-in has changed",
        ));
    }
    let agent = &actor.threads.get(&thread)?.row.agent;
    let code = actor
        .wiring
        .sign_in
        .refused
        .in_force(agent, actor.clock.now().mono)?;
    tracing::info!(%agent, code = code.as_str(), "a launch is refused");
    Some(Error::refused(
        code,
        "Clauseo is not starting this agent for now",
    ))
}

/// Whether `thread`'s launch waits for its agent's `agent.keys_loading` or
/// `agent.key_unreadable` refusal: when it is the refusal in force (no `stop_agents` hold), the
/// wait, at most [`KEYS_WAIT`] on the host clock and woken when the refusal lifts or changes.
/// For `agent.key_unreadable` desktop is first asked to read the key again, so a read that
/// now succeeds lifts it. The launch checks the refusals again after it
/// (`launch_checks::on_prepared`).
pub(super) fn keys_wait(
    actor: &mut Actor,
    thread: ThreadId,
) -> Option<impl std::future::Future<Output = ()> + Send + use<>> {
    if holds(actor, thread) {
        return None;
    }
    let agent = actor.threads.get(&thread)?.row.agent.clone();
    let refused = &actor.wiring.sign_in.refused;
    let code = refused.in_force(&agent, actor.clock.now().mono)?;
    if !WAITED_OUT.contains(&code) {
        return None;
    }
    if code == ErrorCode::AgentKeyUnreadable {
        tracing::info!(%agent, "a launch waits for its stored key to be read again");
        refused.retry_key(&agent);
    } else {
        tracing::info!(%agent, "a launch waits for the stored keys to load");
    }
    let timer = actor.clock.after(KEYS_WAIT);
    Some(actor.wiring.sign_in.refused.wait(&agent, timer))
}

/// `set_launch_refused`: keeps or clears the refusal of `agent`'s new launches; a code not on
/// the list is refused with `command.invalid` and changes nothing. A launch waiting on an
/// `agent.keys_loading` refusal is woken when it lifts or changes.
fn set_refused(actor: &mut Actor, agent: AgentId, code: Option<ErrorCode>) -> Result<(), Error> {
    let shown = agent.clone();
    actor.wiring.sign_in.refused.set(agent, code)?;
    if let Some(code) = code {
        tracing::info!(agent = %shown, code = code.as_str(), "new launches are refused");
    } else {
        tracing::info!(agent = %shown, "new launches may start");
    }
    Ok(())
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
    let Some(checked) = prepared.codex.as_ref().and_then(|codex| codex.login) else {
        return Ok(());
    };
    let seen = checked.link;
    let Some(agent) = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone())
    else {
        return Ok(());
    };
    let sign_in = &mut actor.wiring.sign_in;
    sign_in.seen.insert(agent.clone(), checked);
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
