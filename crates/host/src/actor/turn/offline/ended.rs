//! How a prompt that ended (an error, an answer carrying a `connection` failure) fails for its
//! connection (`offline.rs`): the verdict's code (`hold.rs`: `agent.offline`,
//! `agent.unreachable` or, for a slow service, `agent.quiet`), the agent's own code, or neither.

use clauseo_acp::{ConnectionLost, PromptEnd};
use clauseo_protocol::{ErrorCode, ThreadId};

use super::super::super::Actor;
use super::super::{current, sign_in};
use super::probe::Reach;
use super::{Arm, classify, hold};

/// Whether a prompt of `thread` that ended `result` failed for its connection, as
/// [`prompt_failed_offline`] and [`connection_ended_offline`] read it: `Some(timed_out)`, true
/// when the failure is a request that timed out. A prompt refused as too long never is.
pub(super) fn candidate(
    actor: &mut Actor,
    thread: ThreadId,
    result: &Result<PromptEnd, clauseo_acp::Error>,
) -> Option<bool> {
    match result {
        Err(error) if !error.prompt_too_long() => failed_for_connection(actor, thread, error),
        Ok(PromptEnd {
            connection: Some(lost),
            too_long: None,
            ..
        }) => lost_for_connection(actor, thread, lost),
        _ => None,
    }
}

/// The code a failed prompt of `thread` ends with when it failed for its connection: the
/// verdict's (`hold.rs`), which the host's probe and the window decide.
fn prompt_failed_offline(
    actor: &mut Actor,
    thread: ThreadId,
    error: &clauseo_acp::Error,
) -> Option<ErrorCode> {
    let timed_out = failed_for_connection(actor, thread, error)?;
    Some(hold::verdict(actor, thread, timed_out))
}

/// Whether a failed prompt of `thread` failed for its connection: `Some(timed_out)`. A sign-in
/// the error asks for comes first, and an error that says a server answered (a status, a limit)
/// never did, watch or not; else the error names a connection failure, or an agent's note armed
/// the watch. The RPC error's `data` (codex-acp's `data.message`) is read beside its text.
fn failed_for_connection(
    actor: &mut Actor,
    thread: ThreadId,
    error: &clauseo_acp::Error,
) -> Option<bool> {
    if !matches!(error, clauseo_acp::Error::Rpc { .. }) {
        return None;
    }
    let text = error.to_string();
    let data = error.rpc_data();
    if sign_in::code_of(actor, error.code(), &text, data) == ErrorCode::AgentAuthRequired {
        return None;
    }
    let both = [Some(text.as_str()), data];
    let texts = || both.iter().flatten();
    let server = texts().any(|text| classify::says_server(text));
    let connection = texts().any(|text| classify::says_connection(text));
    // Only an agent's note counts: the window's word alone does not make any failure offline.
    let noted = current(actor, thread)
        .and_then(|current| current.offline.as_ref())
        .is_some_and(|watch| watch.by == Arm::Note);
    let timed_out = texts().any(|text| classify::says_timeout(text));
    (!server && (noted || connection)).then_some(timed_out)
}

/// Whether a prompt of `thread` that ended on a `connection` failure the agent gave up on
/// (codex-acp's, once Codex's retries ran out) failed for its connection: `Some(timed_out)`
/// when an agent's note armed the watch or the title names a connection failure, unless the
/// title says a server answered. A failure the agent did not give up on never did here.
fn lost_for_connection(actor: &mut Actor, thread: ThreadId, lost: &ConnectionLost) -> Option<bool> {
    if !lost.gave_up {
        return None;
    }
    let title = lost.title.as_deref().unwrap_or_default();
    let noted = current(actor, thread)
        .and_then(|current| current.offline.as_ref())
        .is_some_and(|watch| watch.by == Arm::Note);
    (!classify::says_server(title) && (noted || classify::says_connection(title)))
        .then(|| classify::says_timeout(title))
}

/// The code a prompt of `thread` that ended on a `connection` failure the agent gave up on ends
/// with, when it failed for its connection ([`lost_for_connection`]): the verdict's, unless the
/// host's last probe got through (an outage of the service, not of the computer or the route).
/// Else `None`, and the turn fails with the agent's error ([`gave_up_code`]): codex-acp's own
/// "Connection to Codex was lost." when its app-server exits names none.
fn connection_ended_offline(
    actor: &mut Actor,
    thread: ThreadId,
    lost: &ConnectionLost,
) -> Option<ErrorCode> {
    let timed_out = lost_for_connection(actor, thread, lost)?;
    let watch_reached = current(actor, thread)
        .and_then(|current| current.offline.as_ref())
        .is_some_and(|watch| watch.reached);
    let reached = watch_reached || hold::fresh_reach(actor, thread).is_some_and(Reach::reached);
    let code = (!reached).then(|| hold::verdict(actor, thread, timed_out));
    tracing::info!(%thread, reached, ?code, "the prompt ended on a lost connection");
    code
}

/// The code a prompt that ended on a `connection` failure the agent gave up on, and that is not
/// offline, fails with: the one the same failure as a JSON-RPC error gets
/// (`agent.protocol_error`), so the cut-off answer is never shown as complete and Try again
/// stands. Not `agent.exited`: the adapter is still there.
fn gave_up_code(lost: &ConnectionLost) -> Option<ErrorCode> {
    lost.gave_up.then_some(ErrorCode::AgentProtocolError)
}

/// How a prompt of `thread` that ended `result` fails for its connection: `(offline, lost)`. An
/// error is offline by [`prompt_failed_offline`]. An answer that carried a `connection` failure
/// fails offline by [`connection_ended_offline`], else once the agent gave up by
/// [`gave_up_code`], its title the detail. A turn that ends `TooLong` (`too_long`) does neither.
pub(in crate::actor::turn) fn prompt_ended(
    actor: &mut Actor,
    thread: ThreadId,
    result: &Result<PromptEnd, clauseo_acp::Error>,
    too_long: bool,
) -> (Option<ErrorCode>, Option<(ErrorCode, Option<String>)>) {
    match result {
        _ if too_long => (None, None),
        Err(error) => (prompt_failed_offline(actor, thread, error), None),
        Ok(PromptEnd {
            connection: Some(lost),
            ..
        }) => {
            let code = connection_ended_offline(actor, thread, lost).or_else(|| gave_up_code(lost));
            (None, code.map(|code| (code, lost.title.clone())))
        }
        Ok(_) => (None, None),
    }
}
