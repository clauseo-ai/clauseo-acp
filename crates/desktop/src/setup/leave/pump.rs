//! One S18 sign-in state from the pump ([`signin_event`]).
//!
//! - A resting state goes to the agent's waiter first, when one was put before S18 sent it.
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
use super::{Waiter, after_stop, login_changed, restart_for_login, set_reused};

/// See the module docs.
pub(crate) async fn signin_event(backend: &AgentsBackend, agent: &AgentId, state: SigninState) {
    let before = backend.compose.s18(agent);
    let mut found_hold = None;
    match backend.wiring.waiters.deliver(agent, &state) {
        Some(Waiter::Leave(tell)) => {
            // The leave stopped waiting: its own timeout said so.
            let _gone = tell.send(state.clone());
            backend.compose.signin(agent, state);
            return;
        }
        Some(Waiter::Found(hold)) => found_hold = Some(hold),
        None => {}
    }
    let was_reused = matches!(
        &before,
        Some(SigninState::SignedIn {
            source: Source::Reused,
            ..
        })
    );
    let resting = !is_running(&state);
    let reuse_ended = was_reused
        && resting
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
    if let SigninState::SignedIn { email, source, .. } = &state
        && !matches!(
            &before,
            Some(SigninState::SignedIn { email: was, source: had, .. })
                if was == email && had == source
        )
    {
        backend.wiring.leaves.signed_in(agent);
        backend.compose.end_switching(agent);
        signed_in(backend, agent, *source, before.as_ref(), reuse_ended).await;
        drop(found_hold.take());
        backend.compose.signin(agent, state);
        restart_for_login(backend, agent).await;
        return;
    }
    drop(found_hold);
    let signed_out = matches!(&before, Some(SigninState::SignedIn { .. }))
        && resting
        && !matches!(state, SigninState::SignedIn { .. });
    backend.compose.signin(agent, state);
    if reuse_ended {
        login_changed(backend, agent).await;
    } else if signed_out {
        restart_for_login(backend, agent).await;
    }
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
