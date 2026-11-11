//! Codex's shell stays offline: a Codex approval that would give a shell command the network is
//! refused by the host, unseen and unlogged, and the agent is told why once a turn.
//!
//! codex-acp 2.2.2 forwards neither the model's `sandbox_permissions` nor its reason (the reason
//! only in a `JetBrains` Air client's `_meta`, `dist/index.js:32330`), so a plain escalation looks
//! the same as Codex's own dangerous-command prompt (codex rust-v0.162.1
//! `core/src/exec_policy.rs:799-807`, `rm -rf` and the like). A request is a network one
//! ([`asks_for_network`]) only by what codex-acp builds, or by its command word:
//! - a managed-network approval (`networkApprovalContext`): `rawInput.url` beside
//!   `rawInput.command` (`CommandReporter.permission`, :28407-28446), or an option
//!   `apply_network_policy_amendment:<n>` (:32029, :32186);
//! - extra permissions that turn the network on: `rawInput.additionalPermissions.network.enabled`
//!   (a command's) or `rawInput.permissions.network.enabled` (the `request_permissions` tool's,
//!   `SandboxPermissionReporter`, :28282-28296);
//! - a heuristic: a command word of `rawInput.command` (the first word of a `;`, `&&`, `||`,
//!   `|` or `&` part, past `sudo`, `env A=b` and the like, its scripts' too) that is `curl` or
//!   `wget`, bare or with a path (`agent::data_fence::command_names`).
//!
//! It only ever refuses, never allows, so a wrong match costs a command the offline shell could
//! not run anyway. Any other request reaches the lawyer as before. The answer is codex-acp's
//! `decline` ("continue without running it"), else the first reject-once option, else
//! `cancelled`. The note ([`AGENT_NOTE`]) goes as a steer into the running turn, never logged,
//! sent before the answer while the request still holds the turn open, so it cannot start a
//! turn of its own (codex-acp starts one for a steer when no turn runs).

use clauseo_acp::SteerResult;
use clauseo_protocol::{
    AgentChoiceId, AgentKind, ContentBlock, PermissionKind, PermissionOption, ThreadId,
};
use serde_json::Value;

use super::super::Actor;
use super::agents::Stage;
use super::asks::PermissionAsk;
use super::updates::agent;
use super::{current, spawn};
use crate::log_file::Cause;

/// What the agent is told after the first refusal of the turn.
pub(super) const AGENT_NOTE: &str =
    "Clauseo keeps your shell offline: save files with the browser's download tool.";

/// codex-acp's option that declines a command and lets the turn go on (`dist/index.js:32026`).
const DECLINE: &str = "decline";

/// The prefix of codex-acp's network-policy options (`dist/index.js:32029`).
const NETWORK_AMENDMENT: &str = "apply_network_policy_amendment";

/// The command names the heuristic refuses.
const FETCHERS: [&str; 2] = ["curl", "wget"];

/// Refuses the request when the thread's agent is Codex and it asks for the network
/// ([`asks_for_network`]); says whether it did. Called only in the live turn.
pub(super) fn refused(
    actor: &mut Actor,
    thread: ThreadId,
    ask: &PermissionAsk,
    raw_input: Option<&Value>,
) -> bool {
    let kind = actor
        .check_thread(thread)
        .ok()
        .and_then(|entry| AgentKind::of(&entry.row.agent));
    if kind != Some(AgentKind::Codex) || !asks_for_network(raw_input, &ask.options, cfg!(windows)) {
        return false;
    }
    refuse(actor, thread, ask);
    true
}

/// Whether a Codex request asks for the network (see the module doc).
pub(super) fn asks_for_network(
    raw_input: Option<&Value>,
    options: &[PermissionOption],
    windows: bool,
) -> bool {
    let amendment = options
        .iter()
        .any(|option| option.option_id.starts_with(NETWORK_AMENDMENT));
    let Some(input) = raw_input else {
        return amendment;
    };
    let url = input.get("url").is_some_and(Value::is_string) && input.get("command").is_some();
    let enabled = |key: &str| {
        input
            .get(key)
            .and_then(|permissions| permissions.pointer("/network/enabled"))
            == Some(&Value::Bool(true))
    };
    let fetcher = crate::agent::data_fence::command_names(raw_input, windows)
        .iter()
        .any(|name| FETCHERS.contains(&name.as_str()));
    amendment || url || enabled("additionalPermissions") || enabled("permissions") || fetcher
}

/// Answers `decline`, else the reject-once option, else `cancelled`; then, the first time in
/// the turn, sends the note in the same task, so the agent reads it after the refusal.
fn refuse(actor: &mut Actor, thread: ThreadId, ask: &PermissionAsk) {
    let decline = ask
        .options
        .iter()
        .find(|option| option.option_id == DECLINE)
        .or_else(|| reject_option(&ask.options));
    let option = decline.and_then(|option| AgentChoiceId::new(&option.option_id).ok());
    let note = tell_once(actor, thread).then(|| ask.session.clone());
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    let client = agent.link.client.clone();
    let key = ask.key;
    tracing::info!(%thread, "refused a Codex shell command's network");
    spawn(actor, async move {
        // The note goes out first, while the request holds the turn open, so it can only join
        // that turn; the answer follows on the same connection without waiting for it.
        let steer = async {
            let session = note?;
            let text = AGENT_NOTE.to_owned();
            let content = vec![ContentBlock::Text { text }];
            Some(client.steer_held(&session, content).await)
        };
        let answer = async {
            match option {
                Some(option) => client.answer_permission(key, option).await,
                None => client.cancel_permission(key).await,
            }
        };
        let (steered, answered) = futures::future::join(steer, answer).await;
        if let Err(error) = answered {
            tracing::warn!(error = %Cause(&error), "a network refusal did not reach the agent");
        }
        match steered {
            None => {}
            Some(Ok(SteerResult::StartedNewTurn)) => {
                tracing::warn!("the offline-shell note started a turn the host does not track");
            }
            Some(Ok(result)) => {
                tracing::debug!(?result, "the offline-shell note reached the agent");
            }
            Some(Err(error)) => {
                let error = Cause(&error);
                tracing::warn!(%error, "the offline-shell note did not reach the agent");
            }
        }
    });
}

/// The option that rejects this request only: the first `RejectOnce`. A `RejectAlways` is
/// never taken, since it may leave a lasting rule in the agent the lawyer never chose.
pub(super) fn reject_option(options: &[PermissionOption]) -> Option<&PermissionOption> {
    options
        .iter()
        .find(|option| option.kind == PermissionKind::RejectOnce)
}

/// Whether the agent gets the note: the first refusal of the live turn, from an attached agent
/// with native steering (`clauseo_acp::Client::steer_held`). Later refusals of the same turn
/// say nothing more.
fn tell_once(actor: &mut Actor, thread: ThreadId) -> bool {
    let Some(current) = current(actor, thread) else {
        return false;
    };
    if std::mem::replace(&mut current.offline_told, true) {
        return false;
    }
    agent(actor, thread)
        .filter(|agent| agent.stage == Stage::Attached)
        .and_then(|agent| agent.info.as_ref())
        .and_then(|info| info.capabilities.steering)
        .is_some_and(|steering| steering.native)
}

#[cfg(test)]
mod tests;
