//! The sign-in jobs, for host.log: one line on the lifecycle target as each job starts (its
//! agent, its kind and why it runs) and one as it ends (its result and how long it took). The
//! kind is `sign_in`, `sign_out`, `reuse` ("Use it") or `status`. Fixed words only: never an
//! email, a plan, an account, a URL, a path or a token, so a state's fields are never read but
//! its kind and its error code.

use clauseo_protocol::AgentId;

use super::Slot;
use crate::signin::job::Task;
use crate::signin::{Method, SigninState};
use crate::worker::Ticks;

/// The target: host.log keeps info only from `clauseo_*::lifecycle` targets.
pub(crate) const LIFECYCLE: &str = "clauseo_agents::lifecycle";

/// How a job ended, beyond the state it sent last.
#[derive(Debug, Clone, Copy)]
pub(super) enum End<'a> {
    /// It sent this state as its last.
    State(&'a SigninState),
    /// It ended with the state left as it was.
    Unchanged,
    /// A newer job of the agent replaced it.
    Superseded,
    /// The quit or the drop of `Agents` stopped it.
    Stopped,
}

/// `task`'s kind and why it runs.
fn kind(task: Task) -> (&'static str, &'static str) {
    match task {
        Task::SignIn(method) => ("sign_in", method_word(method)),
        Task::TryAgain => ("sign_in", "try_again"),
        Task::UseDifferent => ("sign_in", "use_different_account"),
        Task::SignOut => ("sign_out", "sign_out"),
        Task::StopUsing(None) => ("sign_out", "stop_using"),
        Task::StopUsing(Some(_)) => ("sign_out", "reuse_stopped"),
        Task::UseFound => ("reuse", "use_it"),
        Task::Status => ("status", "check"),
        Task::FindLogin => ("status", "find_login"),
    }
}

fn method_word(method: Method) -> &'static str {
    match method {
        Method::ClaudeAi => "claude_ai",
        Method::Console => "console",
        Method::Browser => "browser",
        Method::DeviceCode => "device_code",
    }
}

/// The name the log gives `state`'s kind.
pub(crate) fn state_name(state: &SigninState) -> &'static str {
    match state {
        SigninState::SignedOut { .. } => "signed_out",
        SigninState::Checking => "checking",
        SigninState::Queued => "queued",
        SigninState::Waiting { .. } => "waiting",
        SigninState::DeviceCode { .. } => "device_code",
        SigninState::LoginFound { .. } => "login_found",
        SigninState::SignedIn { .. } => "signed_in",
        SigninState::Failed { code, .. } if *code == crate::signin::SigninCode::Cancelled => {
            "cancelled"
        }
        SigninState::Failed { .. } => "failed",
    }
}

/// `agent`'s job of `task` started.
pub(super) fn started(agent: &AgentId, task: Task) {
    let (kind, why) = kind(task);
    tracing::info!(target: LIFECYCLE, agent = agent.as_str(), kind, why, "sign-in job started");
}

/// `agent`'s job, `slot`, ended as `end` says.
pub(super) fn ended(ticks: &Ticks, agent: &AgentId, slot: &Slot, end: End<'_>) {
    let (kind, why) = kind(slot.task);
    let took_ms = ticks.since_ms(slot.began);
    let (result, code) = match end {
        End::State(state @ SigninState::Failed { code, .. }) => (state_name(state), code.code()),
        End::State(state) => (state_name(state), "none"),
        End::Unchanged => ("unchanged", "none"),
        End::Superseded => ("superseded", "none"),
        End::Stopped => ("stopped", "none"),
    };
    tracing::info!(target: LIFECYCLE, agent = agent.as_str(), kind, why, result, code, took_ms,
        "sign-in job ended");
}
