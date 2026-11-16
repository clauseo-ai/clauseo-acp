//! Codex's shell stays offline: a Codex approval that would give a shell command the network is
//! refused by the host without asking the lawyer; once a turn the lawyer sees a notice
//! (`NoticeKind::ShellNetworkBlocked`) and the agent is told why.
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
//! not run anyway; so it applies in forced ask and with an unprotected folder too. Any other
//! request reaches the lawyer as before. The answer is codex-acp's
//! `decline` ("continue without running it"), else the first reject-once option, else
//! `cancelled`. The note ([`AGENT_NOTE`]) goes as a steer into the running turn, never logged,
//! sent before the answer while the request still holds the turn open, so it cannot start a
//! turn of its own (codex-acp starts one for a steer when no turn runs).
//!
//! The answer cannot carry the note: codex-acp keeps only the chosen option's id
//! (`selectedDecision`, `dist/index.js:32443-32447`). So when the steer cannot go (no native
//! steering, an agent not attached) or does not land (`PromptRequired`, `Failed`, an error),
//! the note is owed: [`owed_note`] goes first in the thread's next prompt (`prompt.rs`).
//! In a session without web search (`header_tools::web_search_on`), the notice
//! (`ShellNetworkBlockedNoWebSearch`) and the notes offer no web search instead.

use clauseo_acp::SteerResult;
use clauseo_protocol::{
    AgentChoiceId, AgentKind, ContentBlock, Notice, NoticeKind, NoticeLevel, PermissionKind,
    PermissionOption, ThreadId,
};
use serde_json::Value;

use super::super::Actor;
use super::super::input::Input;
use super::agents::Stage;
use super::asks::PermissionAsk;
use super::updates::{agent, commit};
use super::{TurnInput, current, spawn, state};
use crate::log_file::Cause;

/// What the agent is told after the first refusal of the turn, without its way out.
const AGENT_NOTE: &str = "Clauseo declined that command: your shell has no internet.";

/// The note owed when the steer did not land, first in a later prompt (a queued prompt may
/// start first, so it says "earlier"), without its way out.
const OWED_NOTE: &str = "Note from Clauseo: earlier, Clauseo declined a command of yours that \
    needed the internet, because your shell has no internet.";

/// The notes' way out with web search.
const WITH_SEARCH: &str = "Use your web search or web fetch tool instead, or, if you have the \
    browser, save a file by clicking its link there.";

/// The notes' way out without web search.
const WITHOUT_SEARCH: &str = "If you have the browser, save a file by clicking its link there.";

/// The notice's fallback words, for a view that does not know its kind (TN-13's English with
/// `ChatGPT`, the name the app gives Codex; the app words the kind itself).
const NOTICE_TEXT: &str = "Clauseo stopped a step: ChatGPT tried to go online by running a \
    command. ChatGPT can use web search instead.";

/// [`NOTICE_TEXT`] without web search (UC-570's English with `ChatGPT`).
const NOTICE_TEXT_NO_SEARCH: &str =
    "Clauseo stopped a step: ChatGPT tried to go online by running a command.";

/// The steered note, its way out as `web_search` allows.
pub(super) fn agent_note(web_search: bool) -> String {
    format!("{AGENT_NOTE} {}", way_out(web_search))
}

/// The owed note, its way out as `web_search` allows.
pub(super) fn owed_note(web_search: bool) -> String {
    format!("{OWED_NOTE} {}", way_out(web_search))
}

fn way_out(web_search: bool) -> &'static str {
    if web_search {
        WITH_SEARCH
    } else {
        WITHOUT_SEARCH
    }
}

/// The notice for a session with web search or without.
pub(super) fn notice(web_search: bool) -> Notice {
    let (text, kind) = if web_search {
        (NOTICE_TEXT, NoticeKind::ShellNetworkBlocked)
    } else {
        (
            NOTICE_TEXT_NO_SEARCH,
            NoticeKind::ShellNetworkBlockedNoWebSearch,
        )
    };
    Notice {
        level: NoticeLevel::Warning,
        text: text.to_owned(),
        kind: Some(kind),
    }
}

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

/// Answers `decline`, else the reject-once option, else `cancelled`. The first time in the turn
/// it logs the notice and sends the note in the same task, so the agent reads it after the
/// refusal; a note that does not land is owed ([`owe_note`]).
fn refuse(actor: &mut Actor, thread: ThreadId, ask: &PermissionAsk) {
    let decline = ask
        .options
        .iter()
        .find(|option| option.option_id == DECLINE)
        .or_else(|| reject_option(&ask.options));
    let option = decline.and_then(|option| AgentChoiceId::new(&option.option_id).ok());
    let note = match tell_once(actor, thread) {
        Some(Route::Steer) => Some(ask.session.clone()),
        Some(Route::Owe) => {
            owe_note(actor, thread);
            None
        }
        None => None,
    };
    let post = actor.post.clone();
    let text = agent_note(crate::actor::header_tools::web_search_on(actor, thread));
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
        let landed = match steered {
            None => return,
            Some(Ok(SteerResult::StartedNewTurn)) => {
                tracing::warn!("the offline-shell note started a turn the host does not track");
                true
            }
            Some(Ok(SteerResult::Injected)) => {
                tracing::debug!("the offline-shell note reached the agent");
                true
            }
            Some(Ok(result)) => {
                tracing::debug!(?result, "the offline-shell note is owed to the next prompt");
                false
            }
            Some(Err(error)) => {
                let error = Cause(&error);
                tracing::warn!(%error, "the offline-shell note is owed to the next prompt");
                false
            }
        };
        if !landed {
            let _gone = post.try_send(Input::Turn(TurnInput::OfflineNoteOwed(thread)));
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

/// How the turn's first refusal tells the agent.
#[derive(Debug, PartialEq, Eq)]
enum Route {
    /// A steer into the running turn (`clauseo_acp::Client::steer_held`).
    Steer,
    /// First in the next prompt: the agent cannot take the steer.
    Owe,
}

/// The first refusal of the live turn logs the notice and says how the agent is told: a steer
/// to an attached agent with native steering, else owed. Later refusals of the same turn (the
/// agent retrying) say nothing more (`None`).
fn tell_once(actor: &mut Actor, thread: ThreadId) -> Option<Route> {
    let current = current(actor, thread)?;
    if std::mem::replace(&mut current.told.offline, true) {
        return None;
    }
    let notice = notice(crate::actor::header_tools::web_search_on(actor, thread));
    if let Err(error) = commit(actor, thread, [notice.into()], Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "a shell-network notice was not logged");
    }
    let native = agent(actor, thread)
        .filter(|agent| agent.stage == Stage::Attached)
        .and_then(|agent| agent.info.as_ref())
        .and_then(|info| info.capabilities.steering)
        .is_some_and(|steering| steering.native);
    Some(if native { Route::Steer } else { Route::Owe })
}

/// The note did not reach the agent: [`owed_note`] goes first in the thread's next prompt.
pub(super) fn owe_note(actor: &mut Actor, thread: ThreadId) {
    if let Some(state) = state(actor, thread) {
        state.offline_owed = true;
    }
}

#[cfg(test)]
mod tests;
