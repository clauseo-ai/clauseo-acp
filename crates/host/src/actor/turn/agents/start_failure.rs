//! A launch that failed after the agent started ([`launch_failed`]), and the log line about an
//! agent that ended before its session was ready ([`log_start_tail`]).
//!
//! The adapter prints its session-start phases to stderr (claude-agent-acp 0.89.1:
//! `[session/create] sessionId=… phase=validate-cwd durationMs=1 totalMs=1`, then `settings`,
//! `prepare-query`, `sdk-initialize`, …), so the phases it reached tell where a start failed.
//! Decision record 0047 keeps agent output, stderr tails included, out of the log file, so the
//! line holds only those phase lines (ids, phase names and durations) and the tail's last line,
//! the one most likely to name the failure; the process crate has already redacted the tail
//! (the home folder as `~`, secret values, `sk-` words, bearer tokens). At most
//! [`START_TAIL_BYTES`] of it go in; it never goes to an event or the wire.

use clauseo_protocol::{DetachReason, ErrorCode, ThreadId};

use super::{Stage, fail_waiting, state};
use crate::actor::Actor;
use crate::error::Error;
use crate::log_file::Cause;

/// The most of the stderr summary the log line holds.
pub(crate) const START_TAIL_BYTES: usize = 2048;

/// The prefix of claude-agent-acp's session-start phase lines.
const PHASE_LINE: &str = "[session/create] ";

/// The launch failed after the agent started: stop it and fail what waits for it. An error the
/// agent answered is logged with the request's method, its code, its message's length and its
/// `data`'s key names, never the message or a value. The agent's end then logs its stderr
/// summary ([`log_start_tail`]).
pub(in super::super) fn launch_failed(actor: &mut Actor, thread: ThreadId, error: &Error) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    if let Error::Acp(clauseo_acp::Error::Rpc {
        method,
        code,
        message,
        data,
    }) = error
    {
        tracing::warn!(
            %thread,
            error = %Cause(error),
            method = %method,
            code,
            message_len = message.len(),
            data = %data,
            "the agent did not start: it answered a startup request with an error"
        );
    } else {
        tracing::warn!(%thread, error = %Cause(error), "the agent did not start");
    }
    // Claude's "`cwd` does not exist" answer to `session/new` is `matter.folder_permission`.
    let refusal = crate::agent::folder_access::session_refusal(error);
    let error = refusal.as_ref().unwrap_or(error);
    agent.link.stop();
    agent.stage = Stage::Stopping;
    agent.start_failure = Some(error.code());
    let waiting = std::mem::take(&mut agent.waiting);
    for item in waiting {
        fail_waiting(actor, thread, &item, error);
    }
}

/// Logs, at WARN, the stderr summary of an agent that ended before its session was ready
/// ([`start_summary`]), with the code its launch failed with (`None` when it ended on its own);
/// nothing when `reason` carries no stderr tail.
pub(super) fn log_start_tail(thread: ThreadId, failure: Option<ErrorCode>, reason: &DetachReason) {
    let DetachReason::Exited { code, stderr_tail } = reason else {
        return;
    };
    let summary = start_summary(stderr_tail);
    tracing::warn!(
        %thread,
        failure = failure.as_ref().map_or("none", ErrorCode::as_str),
        ?code,
        stderr = %summary,
        "the agent ended before its session was ready; its start phases and last stderr line"
    );
}

/// The phase lines of `tail` and its last non-empty line (once, when it is a phase line), joined
/// with ` | `, cut to [`START_TAIL_BYTES`] on a character boundary; `none` for an empty tail.
pub(crate) fn start_summary(tail: &str) -> String {
    let lines: Vec<&str> = tail
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect();
    let mut kept: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| line.starts_with(PHASE_LINE) && line.contains(" phase="))
        .collect();
    if let Some(last) = lines.last()
        && kept.last() != Some(last)
    {
        kept.push(last);
    }
    if kept.is_empty() {
        return "none".to_owned();
    }
    let mut summary = kept.join(" | ");
    if summary.len() > START_TAIL_BYTES {
        let mut end = START_TAIL_BYTES;
        while !summary.is_char_boundary(end) {
            end -= 1;
        }
        summary.truncate(end);
    }
    summary
}

#[cfg(test)]
mod tests;
