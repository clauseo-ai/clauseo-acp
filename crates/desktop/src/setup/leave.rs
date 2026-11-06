//! Leaving a reused login, and what follows a login change (decision record 0048, ruling 1; the
//! plan's piece B; the P5b section). One try of the leave order ([`attempt`]) runs:
//!
//! 1. the host's `stop_agents`, which holds the id until `restart_agents`;
//! 2. S18's `stop_using_login` (no-reuse and the home's link, one job), then S18's `SignedOut`,
//!    which the pump hands over ([`Waiters`]) within [`LEAVE_WAIT`](hold::LEAVE_WAIT);
//! 3. [`after_stop`]: `set_reused_login(false)`, `remove_login_links` (Codex), and the entry
//!    rewritten as `Own`, and the readers with it.
//!
//! Around it the leave holds the agent's limits reads (desktop keeps the [`ReadHold`]), then drops
//! them and sends `limits_login_changed`. One leave runs per agent ([`Leaves`]): a second one
//! answers `command.conflict`.
//!
//! **A lawyer's leave** ([`leave_reused`]: `StopUsingLogin`, `UseDifferentAccount`, a key or an
//! endpoint set on a reused login) whose `stop_agents` times out, or whose call fails, changes
//! nothing: `restart_agents` lifts the hold, the reads' guard drops, and the command answers
//! `signin.agents_still_running` or the call's error. With no `SignedOut` in 30 s, S18's stop
//! job may still end the reuse and S18 cannot cancel it: the holds stay, with the pump
//! ([`Late`](hold::Late)), until the job ends, and the agent shows `Switching` (`stuck` after
//! 2 minutes). Then the steps after the stop run if it stopped (not on a `Failed`), `Switching`
//! ends, and `restart_agents` lifts the hold (review A2). The agent's slot stays taken till
//! then. `StopUsingLogin` answers done; the commands that leave first ([`leave_first`])
//! answer `command.conflict`, with nothing stored or started. A leave that finishes, at once or
//! late, follows one rule (`after.rs`): `restart_agents` when a stored key, an endpoint or
//! Clauseo's own login remains, else the hold stays, the agent shown signed out, until the new
//! login's `SignedIn` the pump sees. A leave that is a key's or an endpoint's first step leaves
//! the restart to its command, once the key or the endpoint is stored ([`After::Command`]). A
//! new `SignedIn` that lands during the leave ends it as done: the pump has handled it,
//! `restart_agents` included (review A1).
//!
//! **An automatic leave** (`auto.rs`: a split, or a reuse S18 ended by itself) that does not
//! finish keeps the hold and tries again, showing `Switching`. It never asks S18 to stop once a
//! new login took over, nor while a sign-in runs ([`taken_over`]).

mod after;
mod auto;
mod hold;
mod pump;
mod slots;
mod waiters;

use clauseo_agents::{Found, SigninState, Source};
use clauseo_host::AgentsStopped;
use clauseo_protocol::{AgentId, AgentKind, ErrorCode, HostError, SignInSource};
use futures::channel::oneshot;
use futures::future::{Either, select};

pub(crate) use self::after::{After, restart_if_usable};
#[cfg(test)]
pub(crate) use self::auto::auto_leave;
pub(crate) use self::auto::follow as follow_auto_leaves;
use self::hold::{HostHold, Late};
pub(crate) use self::hold::{LeaveTimer, Wait, retry_after};
use self::pump::is_new_sign_in;
pub(crate) use self::pump::signin_event;
pub(crate) use self::slots::Leaves;
use self::waiters::Put;
pub(crate) use self::waiters::{Ended, Token, Waiter, Waiters};
use super::backend::{AgentsBackend, signin_error};
use super::compose::is_running;
use super::readers::ReadHold;
use crate::bridge::lock;

/// Why one try of the leave order did not finish; nothing of the login changed.
pub(crate) enum NotLeft {
    /// `stop_agents` answered `TimedOut`: an agent process outlived the wait.
    StillRunning,
    /// S18's `SignedOut` did not come within [`LEAVE_WAIT`](hold::LEAVE_WAIT).
    NotStopped,
    /// A new `SignedIn` came instead, and the pump has handled it, `restart_agents` included.
    NewSignIn,
    /// An automatic leave found the login's place taken before it asked S18 to stop
    /// ([`taken_over`]); it asked nothing.
    TakenOver(Over),
    /// A call failed.
    Error(HostError),
}

/// Whose leave a try is for.
pub(crate) enum Whose<'a> {
    /// A lawyer's: when the 30 s wait runs out, its holds go to the pump in place of its waiter
    /// ([`Waiter::Late`]), leaving `None` here.
    Lawyer(&'a mut Option<Late>),
    /// An automatic leave's, which never asks S18 to stop once something took over.
    Auto,
}

impl NotLeft {
    /// The answer a lawyer's command gets.
    fn into_error(self, agent: &AgentId) -> HostError {
        match self {
            Self::StillRunning => HostError::new(
                ErrorCode::SigninAgentsStillRunning,
                format!("an agent of {agent} is still running; the login was not changed"),
            ),
            Self::NotStopped => HostError::new(
                ErrorCode::SigninFailed,
                format!("{agent}'s login you already had did not stop; try again"),
            ),
            Self::NewSignIn | Self::TakenOver(_) => HostError::new(
                ErrorCode::CommandConflict,
                format!("{agent} signed in meanwhile"),
            ),
            Self::Error(error) => error,
        }
    }

    /// Its code, for the logs.
    fn code(&self) -> ErrorCode {
        match self {
            Self::StillRunning => ErrorCode::SigninAgentsStillRunning,
            Self::NotStopped => ErrorCode::SigninFailed,
            Self::NewSignIn | Self::TakenOver(_) => ErrorCode::CommandConflict,
            Self::Error(error) => error.code(),
        }
    }
}

/// How a lawyer's leave that did not fail ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Left {
    /// The login was left, or a new sign-in took its place.
    Done,
    /// It stopped waiting for S18's stop job, which may still end the reuse: the agent shows
    /// `Switching` and stays held until the job ends.
    Pending,
}

/// A lawyer's leave of `agent`'s reused login, as the first step of another command
/// (`UseDifferentAccount`, a key or an endpoint): one that is still pending answers
/// `command.conflict`, with nothing stored or started, since S18's stop job may still end the
/// reuse or fail, and rewrite the entry under the command's change. `after`: who restarts.
pub(crate) async fn leave_first(
    backend: &AgentsBackend,
    agent: &AgentId,
    after: After,
) -> Result<(), HostError> {
    match leave_reused(backend, agent, after).await? {
        Left::Done => Ok(()),
        Left::Pending => Err(HostError::new(
            ErrorCode::CommandConflict,
            format!("still leaving {agent}'s old login; try again in a moment"),
        )),
    }
}

/// A lawyer's leave of `agent`'s reused login: see the module docs. Once it finished, the
/// agents restart by the rule (`after.rs`), unless its command restarts them (`after`).
pub(crate) async fn leave_reused(
    backend: &AgentsBackend,
    agent: &AgentId,
    after: After,
) -> Result<Left, HostError> {
    let Some(kind) = AgentKind::of(agent) else {
        return Err(HostError::new(ErrorCode::CommandInvalid, "no such agent"));
    };
    let mut slot = backend.wiring.leaves.begin(agent, &backend.host)?;
    tracing::info!(%agent, "leaving a reused login");
    let reads = backend.wiring.readers.hold(kind).await;
    let hold = HostHold::new(std::sync::Arc::clone(&backend.host), agent.clone());
    // On a panic `Late` drops its fields in order: the host's hold, then the reads'.
    let mut late = Some(Late::new(hold, reads));
    let tried = attempt(backend, agent, kind, None, Whose::Lawyer(&mut late)).await;
    let Some(Late { hold, reads, .. }) = late else {
        tracing::info!(%agent, "a leave stopped waiting; its hold stays until S18's job ends");
        slot.linger();
        return Ok(Left::Pending);
    };
    let result = match tried {
        Ok(first_error) => {
            slot.left();
            drop(reads);
            // A command that fails here stops: the rule decides for it.
            if after == After::Rule || first_error.is_some() {
                self::after::finish(backend, agent, hold).await;
            } else {
                hold.keep();
            }
            login_changed(backend, agent).await;
            first_error.map_or(Ok(Left::Done), Err)
        }
        Err(NotLeft::NewSignIn) => {
            tracing::info!(%agent, "a new sign-in took the place of a login being left");
            hold.keep();
            slot.left();
            drop(reads);
            Ok(Left::Done)
        }
        Err(not_left) => {
            tracing::info!(%agent, code = not_left.code().as_str(), "a leave did not finish");
            hold.lift(backend).await;
            drop(reads);
            Err(not_left.into_error(agent))
        }
    };
    // The slot frees here on every end, and an endpoint that arrived meanwhile is restarted.
    if slot.free() {
        restart_if_usable(backend, agent).await;
    }
    result
}

/// One try of the leave order (module docs, steps 1 to 3). `Ok` carries the first error of
/// step 3, whose steps all run.
pub(crate) async fn attempt(
    backend: &AgentsBackend,
    agent: &AgentId,
    kind: AgentKind,
    why: Option<Found>,
    whose: Whose<'_>,
) -> Result<Option<HostError>, NotLeft> {
    match backend.host.stop_agents(agent.clone()).await {
        Ok(AgentsStopped::Exited) => {}
        Ok(AgentsStopped::TimedOut) => return Err(NotLeft::StillRunning),
        Err(error) => return Err(NotLeft::Error(error)),
    }
    let (tell, stopped) = oneshot::channel();
    let Some(token) = wait_on(backend, agent, Waiter::Leave(tell)) else {
        return Err(NotLeft::Error(HostError::new(
            ErrorCode::CommandConflict,
            format!("{agent}'s login is already being left"),
        )));
    };
    let _put = Put {
        waiters: &backend.wiring.waiters,
        agent: agent.clone(),
        token,
    };
    ask_to_stop(backend, agent, why, matches!(whose, Whose::Auto))?;
    let wait = backend.wiring.timer.after(Wait::SignedOut);
    let ended = match select(stopped, wait).await {
        Either::Left((Ok(ended), _)) => Some(ended),
        Either::Left((Err(_), _)) => None,
        Either::Right(((), stopped)) => give_up(backend, agent, token, whose, stopped).await,
    };
    match ended {
        Some(Ended::SignedOut) => Ok(after_stop(backend, agent, kind).await),
        Some(Ended::NewSignIn) => Err(NotLeft::NewSignIn),
        Some(Ended::Other) | None => Err(NotLeft::NotStopped),
    }
}

/// S18's `stop_using_login`, under the sign-in gate, so no sign-in of desktop's starts between
/// the check and the call: for an automatic leave, only when nothing took over (`stop_using_login`
/// replaces a running sign-in, and fails on Clauseo's own login).
fn ask_to_stop(
    backend: &AgentsBackend,
    agent: &AgentId,
    why: Option<Found>,
    auto: bool,
) -> Result<(), NotLeft> {
    let _gate = lock(&backend.signin_gate);
    if auto && let Some(over) = taken_over(backend, agent) {
        return Err(NotLeft::TakenOver(over));
    }
    backend
        .agents
        .stop_using_login(agent, why)
        .map_err(|error| NotLeft::Error(signin_error(&error)))
}

/// The 30 s wait ran out. A lawyer's leave hands its holds to the pump in place of its waiter;
/// when the pump took the waiter just now, what it hands over is awaited instead.
async fn give_up(
    backend: &AgentsBackend,
    agent: &AgentId,
    token: Token,
    whose: Whose<'_>,
    stopped: oneshot::Receiver<Ended>,
) -> Option<Ended> {
    let Whose::Lawyer(late) = whose else {
        return None;
    };
    let given = late.take()?;
    // Before the pump can take it, so its end of `Switching` comes after this start.
    backend.compose.start_switching(agent);
    let alive = given.alive();
    let waiters = &backend.wiring.waiters;
    match waiters.replace(agent, token, Waiter::Late(given)) {
        Ok(()) => {
            watch_stuck(backend, agent, alive);
            None
        }
        Err(waiter) => {
            backend.compose.end_switching(agent);
            if let Waiter::Late(given) = waiter {
                *late = Some(given);
            }
            stopped.await.ok()
        }
    }
}

/// `stuck` for a leave given up, 2 minutes on, while its holds are still with the pump
/// (`alive`): on a thread of its own, since the lawyer's command has answered by then.
fn watch_stuck(backend: &AgentsBackend, agent: &AgentId, alive: std::sync::Weak<()>) {
    let wait = backend.wiring.timer.after(Wait::Stuck);
    let compose = std::sync::Arc::clone(&backend.compose);
    let for_thread = agent.clone();
    let spawned = std::thread::Builder::new()
        .name("clauseo-setup-stuck".to_owned())
        .spawn(move || {
            futures::executor::block_on(wait);
            if alive.upgrade().is_some() {
                compose.set_stuck(&for_thread);
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%agent, %err, "a leave given up will not show stuck: no thread");
    }
}

/// What took the reused login's place, for an automatic leave (review A1, H1′).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Over {
    /// A new login: Clauseo's own signed in, a new `SignedIn` the pump handled since the leave
    /// began, or one S18 sent that the pump has not taken yet. The leave ends.
    SignedIn,
    /// A sign-in S18 runs or has reserved: the leave asks S18 nothing while it runs, and tries
    /// again after it, should it fail.
    Running,
}

/// Whether, and by what, the reused login's place was taken: an automatic leave then asks S18
/// nothing (review A1, H1′).
pub(crate) fn taken_over(backend: &AgentsBackend, agent: &AgentId) -> Option<Over> {
    let latest = backend.agents.signin_state(agent);
    let own = matches!(
        latest,
        Some(SigninState::SignedIn {
            source: Source::Own,
            ..
        })
    );
    let unseen = latest
        .as_ref()
        .is_some_and(|latest| is_new_sign_in(backend.compose.s18(agent).as_ref(), latest));
    if own || unseen || backend.wiring.leaves.told_signed_in(agent) {
        return Some(Over::SignedIn);
    }
    let running = latest.as_ref().is_some_and(is_running);
    (running || backend.agents.sign_in_going(agent)).then_some(Over::Running)
}

/// The steps after S18 stopped the reuse: `set_reused_login(false)`, `remove_login_links`
/// (Codex), the entry rewritten as `Own`. All run; the first error comes back.
pub(crate) async fn after_stop(
    backend: &AgentsBackend,
    agent: &AgentId,
    kind: AgentKind,
) -> Option<HostError> {
    let mut first_error = set_reused(backend, agent, false).await.err();
    if kind == AgentKind::Codex
        && let Err(error) = backend.host.remove_login_links(agent.clone()).await
    {
        tracing::warn!(%agent, code = error.code().as_str(), "matter links not removed");
        first_error.get_or_insert(error);
    }
    backend.write_entry_as(agent, SignInSource::Own).await;
    first_error
}

/// Puts `waiter` for `agent` with S18's latest state (see [`Waiters::put`]).
pub(crate) fn wait_on(backend: &AgentsBackend, agent: &AgentId, waiter: Waiter) -> Option<Token> {
    let latest = backend.agents.signin_state(agent);
    backend.wiring.waiters.put(agent, waiter, latest)
}

/// Tells the host, and the login check, whether `agent`'s login is reused.
pub(crate) async fn set_reused(
    backend: &AgentsBackend,
    agent: &AgentId,
    reused: bool,
) -> Result<(), HostError> {
    backend.wiring.reused.set(agent, reused);
    backend
        .host
        .set_reused_login(agent.clone(), reused)
        .await
        .inspect_err(|error| {
            tracing::warn!(%agent, code = error.code().as_str(), "reused login not told");
        })
}

/// `restart_agents` for `agent`, logged by code when it fails. Its launch refusal is set first
/// (`refusal::sync`), so a key that just turned unreadable (a `reload` that failed, whose note
/// the follower has not taken yet) is refused before any launch the restart makes (review L4).
pub(crate) async fn restart(backend: &AgentsBackend, agent: &AgentId) {
    super::refusal::sync(backend, agent).await;
    if let Err(error) = backend.host.restart_agents(agent.clone()).await {
        tracing::warn!(%agent, code = error.code().as_str(), "agents not restarted");
    }
}

/// `limits_login_changed` for `agent`, logged by code when it fails.
pub(crate) async fn login_changed(backend: &AgentsBackend, agent: &AgentId) {
    if let Err(error) = backend.host.login_changed(agent.clone()).await {
        tracing::warn!(%agent, code = error.code().as_str(), "limits not told of the login");
    }
}

/// A key or an endpoint changed: the agents restart with it, and the limits read it. The
/// identity check's exemption is worked out again from the launch env first (R17), so a launch
/// env that no longer replaces the shared login puts `agent.identity_checking` on the agent
/// before the restart relaunches it.
pub(crate) async fn restart_for_login(backend: &AgentsBackend, agent: &AgentId) {
    super::identity::exemption_changed(backend, agent).await;
    restart(backend, agent).await;
    login_changed(backend, agent).await;
}

/// The reads' hold for `agent` across a key or an endpoint change, if it has a kind.
pub(crate) async fn hold_reads(backend: &AgentsBackend, agent: &AgentId) -> Option<ReadHold> {
    let kind = AgentKind::of(agent)?;
    Some(backend.wiring.readers.hold(kind).await)
}
