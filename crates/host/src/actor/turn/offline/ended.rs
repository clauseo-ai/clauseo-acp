//! How a prompt that ended (an error, an answer carrying a `connection` failure) fails for its
//! connection (`offline.rs`): `agent.offline`, the agent's own code, or neither.

use clauseo_acp::{ConnectionLost, PromptEnd};
use clauseo_protocol::{ErrorCode, ThreadId};

use super::super::super::Actor;
use super::super::{current, sign_in};
use super::{Arm, classify};

/// The code a failed prompt of `thread` ends with when it is offline. A sign-in the error asks
/// for comes first, and an error that says a server answered (a status, a limit) is never
/// offline, watch or not; else the error names a connection failure, or an agent's note armed
/// the watch.
/// The RPC error's `data` (codex-acp's `data.message`) is read beside its text.
fn prompt_failed_offline(
    actor: &mut Actor,
    thread: ThreadId,
    error: &clauseo_acp::Error,
) -> Option<ErrorCode> {
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
    (!server && (noted || connection)).then_some(ErrorCode::AgentOffline)
}

/// The code a prompt of `thread` that ended on a `connection` failure the agent gave up on
/// (codex-acp's, once Codex's retries ran out) ends with, when it is offline: `agent.offline`
/// when an agent's note armed the watch or the title names a connection failure, unless the
/// title says a server answered or the host's last probe got through (an outage of the service,
/// not of the computer). Else `None`, and the turn fails with the agent's error
/// ([`gave_up_code`]): codex-acp's own "Connection to Codex was lost." when its app-server
/// exits names none. A failure the agent did not give up on is never offline here.
fn connection_ended_offline(
    actor: &mut Actor,
    thread: ThreadId,
    lost: &ConnectionLost,
) -> Option<ErrorCode> {
    if !lost.gave_up {
        return None;
    }
    let title = lost.title.as_deref().unwrap_or_default();
    let watch = current(actor, thread).and_then(|current| current.offline.as_ref());
    let noted = watch.is_some_and(|watch| watch.by == Arm::Note);
    let reached = watch.is_some_and(|watch| watch.reached);
    let offline =
        !reached && !classify::says_server(title) && (noted || classify::says_connection(title));
    tracing::info!(%thread, offline, noted, reached, "the prompt ended on a lost connection");
    offline.then_some(ErrorCode::AgentOffline)
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
