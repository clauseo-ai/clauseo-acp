//! One S18 sign-in state from the pump ([`signin_event`]).
//!
//! - A resting state goes to the agent's waiter first, when one was put before S18 sent it. A
//!   new `SignedIn` is handled as below first, and only then is a leave waiting told
//!   (`Ended::NewSignIn`): a sign-in that lands during a leave is never swallowed (review A1).
//! - A lawyer's leave that gave up (`Waiter::Late`): S18's stop job's end is handled as below,
//!   then `Switching` ends and the reads' hold drops, so nothing changes under running agents
//!   (review A2). The host's hold lifts (`restart_agents`) when the stop failed or left the
//!   reuse in place; when the reuse ended, the leave finished, and the rule of `after.rs`
//!   decides, as for any leave that finished; a new sign-in's own `restart_agents` lifted it.
//! - Whether the login is reused is desktop's word, as the host was told (`wiring.reused`), so a
//!   running state between (a job of S18's that replaced a stop) never hides it (review M1).
//! - A `Failed` while the login is reused (a stop job of S18's that failed) is not the reuse's
//!   end: no steps after the stop run, and nothing else changes. One that follows a running
//!   state (the failed job replaced another, a stop maybe) leaves the reuse unknown: S18's
//!   status check tells ([`check_reuse`]), and its answer is handled as below, a leave's holds
//!   waiting for it.
//! - A reuse that ends with no leave waiting (a `SignedIn { Reused }` followed by any other
//!   resting state: a leave's late `SignedOut`, or S18 ending the reuse by itself) runs the
//!   steps after the stop at once (`after_stop`, review H1). When S18 ended it by itself
//!   (`ReusedFailed`, `ReusedChanged`, `ReusedSignedOut`, or `CodexLinkSplit` from its status
//!   check), an automatic leave is queued (ruling 1): it stops the agents, so none goes on with
//!   the account the agent's own CLI now holds, and runs on its own task, not in the pump.
//! - A `SignedIn` that is new (none before, or another account or source) ends any automatic
//!   leave's tries and is handled before it is shown: a source that differs from the one the host
//!   was told is told and the entry rewritten (a Codex login of Clauseo's own on another account
//!   has its matter links removed), the reads' hold `UseFoundLogin` took drops, then
//!   `restart_agents` and `limits_login_changed`.
//! - Clauseo's own login signed out: `restart_agents` and `limits_login_changed` (ruling 3).

use clauseo_agents::{Found, SigninState, Source};
use clauseo_protocol::{AgentId, AgentKind, SignInSource};

use super::super::backend::AgentsBackend;
use super::super::compose::is_running;
use super::after::finish;
use super::hold::Late;
use super::{
    Ended, Waiter, after_stop, login_changed, restart_for_login, restart_if_usable, set_reused,
    signin_error, wait_on,
};

/// See the module docs.
pub(crate) async fn signin_event(backend: &AgentsBackend, agent: &AgentId, state: SigninState) {
    let before = backend.compose.s18(agent);
    let new_sign_in = is_new_sign_in(before.as_ref(), &state);
    let (mut found_hold, mut leave, mut late) = (None, None, None);
    match backend.wiring.waiters.deliver(agent, &state) {
        Some(Waiter::Leave(tell)) if !new_sign_in => {
            let ended = if matches!(state, SigninState::SignedOut { .. }) {
                Ended::SignedOut
            } else {
                Ended::Other
            };
            // The leave stopped waiting: its own timeout said so.
            let _gone = tell.send(ended);
            backend.compose.signin(agent, state);
            return;
        }
        Some(Waiter::Leave(tell)) => leave = Some(tell),
        Some(Waiter::Found(hold)) => found_hold = Some(hold),
        Some(Waiter::Late(given_up)) => late = Some(given_up),
        None => {}
    }
    let was_reused = backend.wiring.reused.get(agent);
    let resting = !is_running(&state);
    // A `Failed` while reused is a job of S18's that failed, such as a stop: the reuse holds,
    // unless the job replaced another (a running state between), when S18's check tells.
    let stop_failed = was_reused && matches!(state, SigninState::Failed { .. });
    let unsure = stop_failed && before.as_ref().is_some_and(is_running);
    let reuse_ended = was_reused
        && resting
        && !stop_failed
        && !matches!(
            &state,
            SigninState::SignedIn {
                source: Source::Reused,
                ..
            }
        );
    if reuse_ended {
        ended_by_itself(backend, agent, &state).await;
    }
    if let SigninState::SignedIn { source, .. } = &state
        && new_sign_in
    {
        backend.compose.end_switching(agent);
        signed_in(backend, agent, *source, before.as_ref(), reuse_ended).await;
        drop(found_hold.take());
        backend.compose.signin(agent, state);
        restart_for_login(backend, agent).await;
        // After the restart: a leave told of the sign-in may lift its hold once more, never first.
        backend.wiring.leaves.signed_in(agent);
        if let Some(Late { hold, reads, .. }) = late {
            // The sign-in's own `restart_agents` lifted it.
            hold.keep();
            drop(reads);
            if backend.wiring.leaves.end_late(agent, true) {
                restart_if_usable(backend, agent).await;
            }
        }
        if let Some(tell) = leave {
            // Gone only when the leave was dropped meanwhile.
            let _gone = tell.send(Ended::NewSignIn);
        }
        return;
    }
    drop(found_hold);
    let signed_out = matches!(&before, Some(SigninState::SignedIn { .. }))
        && resting
        && !stop_failed
        && !matches!(state, SigninState::SignedIn { .. });
    backend.compose.signin(agent, state);
    if unsure {
        late = check_reuse(backend, agent, late);
        if late.is_none() {
            return;
        }
    }
    if let Some(Late { hold, reads, .. }) = late {
        tracing::info!(%agent, "the stop a leave gave up on ended: lifting its hold");
        backend.compose.end_switching(agent);
        if reuse_ended {
            finish(backend, agent, hold).await;
        } else {
            hold.lift(backend).await;
        }
        drop(reads);
        // The arrival of an endpoint since the leave began is restarted here (`Leaves::end_late`).
        #[cfg(test)]
        backend.wiring.leaves.pause("given_up.end").await;
        if backend.wiring.leaves.end_late(agent, reuse_ended) {
            restart_if_usable(backend, agent).await;
        }
    }
    if reuse_ended {
        login_changed(backend, agent).await;
    } else if signed_out {
        restart_for_login(backend, agent).await;
    }
}

/// A `Failed` after a running state while `agent`'s login is reused (review M1): the job that
/// failed replaced another, a stop maybe, so whether the reuse ended is not known. S18's status
/// check tells; its answer comes to [`signin_event`] as any state does, with a leave's holds
/// (`late`) put back to wait for it, `Switching` still shown. `late` comes back when the check
/// did not start, for its holds to lift now as for any failed stop.
fn check_reuse(backend: &AgentsBackend, agent: &AgentId, late: Option<Late>) -> Option<Late> {
    tracing::info!(%agent, "a job that replaced another failed on a reused login: checking");
    let token = match late {
        Some(given) => {
            let token = wait_on(backend, agent, Waiter::Late(given));
            if token.is_none() {
                // Refused only while another leave waits, which its slot rules out; its holds
                // lift from their `Drop`.
                tracing::warn!(%agent, "a leave's holds had nowhere to wait");
            }
            token
        }
        None => None,
    };
    let Err(error) = backend.agents.check_status(agent) else {
        return None;
    };
    let code = signin_error(&error).code();
    tracing::warn!(%agent, code = code.as_str(), "no status check after a failed job");
    match token.and_then(|token| backend.wiring.waiters.take(agent, token)) {
        Some(Waiter::Late(given)) => Some(given),
        _ => None,
    }
}

/// Whether `state` is a new `SignedIn` after `before`: none before, or another account or
/// source.
pub(crate) fn is_new_sign_in(before: Option<&SigninState>, state: &SigninState) -> bool {
    matches!(state, SigninState::SignedIn { .. })
        && !matches!(
            (before, state),
            (
                Some(SigninState::SignedIn { email: was, source: had, .. }),
                SigninState::SignedIn { email, source, .. },
            ) if was == email && had == source
        )
}

/// A reuse that ended with no leave waiting: the steps after the stop, and an automatic leave
/// when S18 ended it by itself.
async fn ended_by_itself(backend: &AgentsBackend, agent: &AgentId, state: &SigninState) {
    let Some(kind) = AgentKind::of(agent) else {
        return;
    };
    tracing::info!(%agent, "a reused login ended with no leave waiting");
    // Logged inside; the leave goes on.
    let _first_error = after_stop(backend, agent, kind).await;
    if let SigninState::SignedOut {
        found:
            Some(
                why @ (Found::ReusedFailed
                | Found::ReusedChanged
                | Found::ReusedSignedOut
                | Found::CodexLinkSplit),
            ),
    } = state
    {
        backend.wiring.leaves.submit(agent, Some(*why));
    }
}

/// A new `SignedIn` of `source`: the host told and the entry rewritten when the source differs
/// from the one it was told; a Codex login of Clauseo's own replacing another has its matter
/// links removed (unless the steps after a reuse's end just did).
async fn signed_in(
    backend: &AgentsBackend,
    agent: &AgentId,
    source: Source,
    before: Option<&SigninState>,
    reuse_ended: bool,
) {
    let reused = source == Source::Reused;
    if reused != backend.wiring.reused.get(agent) {
        // Logged inside; the entry and the restart go on.
        let _told = set_reused(backend, agent, reused).await;
        let source = if reused {
            SignInSource::Reused
        } else {
            SignInSource::Own
        };
        backend.write_entry_as(agent, source).await;
    } else if !reused
        && !reuse_ended
        && AgentKind::of(agent) == Some(AgentKind::Codex)
        && matches!(before, Some(SigninState::SignedIn { .. }))
        && let Err(error) = backend.host.remove_login_links(agent.clone()).await
    {
        tracing::warn!(%agent, code = error.code().as_str(), "matter links not removed");
    }
}
