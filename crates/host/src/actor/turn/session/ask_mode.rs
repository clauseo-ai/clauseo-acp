//! Ask mode for a Claude conversation whose auto-mode checks cannot work: the session runs in
//! `default` (`forced_ask::ASK_MODE`, Claude asks before each change) instead of `auto`, with
//! every deny rule still in its settings, so Claude Code checks them locally and no server
//! context is needed. Nothing is trimmed and no launch is refused. Two ways in:
//!
//! - At launch ([`at_launch`]): the launch's flag settings would give an auto-mode context over
//!   `agent::auto_context::BUDGET` (Claude Code 2.1.293 sends none above 1,040,000 bytes, and
//!   then refuses every `Edit` and `Bash` of the launch). Logged at warn; the notice
//!   `NoticeKind::AskModeTooManyMatters` (UC-491) goes with the attach ([`launch_notice`]).
//! - Detected ([`checks_stopped`]): a tool result, an answer or a notice of a session running in
//!   `auto` carries Claude Code's refusal for want of that context ([`carries_refusal`]), as when
//!   the limit moves. Logged at warn, the notice `NoticeKind::AskModeChecksStopped` (UC-492) is
//!   logged, and the agent is marked to relaunch (`idle::Relaunch::ChecksStopped`): its next
//!   prompt stops it and starts it again, which resumes the session in `default`. From then on
//!   every launch of the conversation runs in `default` for the host's run (the thread's turn
//!   state keeps it); a host started again decides from the estimate once more. A session
//!   already in `default` never triggers it, and a replayed transcript is not read, so the
//!   relaunch cannot loop.
//!
//! Each notice shows at most once per conversation: neither again once the conversation holds
//! it (its loaded view) or the host told it this run; and UC-492 not at all once UC-491 showed.
//! A launch held back for want of a sandbox (`Auto::NoSandbox`) stays in `acceptEdits`, which
//! needs no server context either, with no notice.

use clauseo_acp::{AgentUpdate, MessageRole};
use clauseo_protocol::{
    AgentKind, ContentBlock, Notice, NoticeKind, NoticeLevel, StreamId, TextOrBlob, ThreadId,
    ToolCallContent, ViewEntry,
};
use serde_json::{Map, Value};

use super::super::super::Actor;
use super::super::idle::{Relaunch, mark_relaunch};
use super::super::state;
use super::super::updates::commit;
use super::Auto;
use crate::agent::auto_context::{BUDGET, estimate, settings_of};
use crate::log_file::Cause;

/// The host English of UC-491 (`NoticeKind::AskModeTooManyMatters`), for a view that does not
/// know the kind.
pub(crate) const TOO_MANY_MATTERS: &str = "Claude will ask before each change in this \
     conversation. Clauseo is protecting more matters than Claude's automatic checks can handle \
     at once, so Claude asks you instead.";

/// The host English of UC-492 (`NoticeKind::AskModeChecksStopped`).
pub(crate) const CHECKS_STOPPED: &str = "Claude now asks before each change in this \
     conversation, because its automatic checks stopped working here. Clauseo's own \
     protections still apply.";

/// What Claude Code 2.1.293 writes when it refuses a call for want of the auto-mode context
/// (`[server-classifier] Bash: no server verdict for this call (server_not_requested)`): either
/// part is enough.
pub(crate) const REFUSAL: [&str; 2] = ["no server verdict for this call", "server_not_requested"];

/// A conversation's ask mode, kept with its turn state across launches for the host's run.
#[derive(Debug, Default)]
pub(crate) struct AskMode {
    /// A session of it ran into the refusal: every later launch runs in `default`.
    checks_stopped: bool,
    /// This launch went to `default` over budget; its attach owes UC-491.
    launch_owed: bool,
    /// UC-491 was told this run.
    told_launch: bool,
    /// UC-492 was told this run.
    told_stopped: bool,
}

/// Before `thread`'s Claude session call, with its `_meta` (`meta`) built: holds the launch in
/// `default` ([`Auto::Ask`]) when its settings' estimate is over budget or the conversation's
/// checks stopped earlier this run. Forced ask (already `default`) and a launch held in
/// `acceptEdits` for want of a sandbox are left as they are.
pub(crate) fn at_launch(actor: &mut Actor, thread: ThreadId, meta: &Map<String, Value>) {
    let estimated = settings_of(meta).map_or(0, estimate);
    let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
    let Some(state) = state(actor, thread) else {
        return;
    };
    let over = estimated > BUDGET;
    let Some(agent) = state.agent.as_deref_mut() else {
        return;
    };
    if agent.forced_ask || !(over || state.ask.checks_stopped) {
        return;
    }
    if agent.auto == Auto::NoSandbox {
        tracing::info!(%thread, estimated, "Claude runs in acceptEdits; its auto checks are not used");
        return;
    }
    agent.auto = Auto::Ask;
    if over {
        tracing::warn!(
            matter = ?matter,
            estimated,
            budget = BUDGET,
            "Claude's auto-mode context would be too large; the session runs in ask mode"
        );
        state.ask.launch_owed = true;
    } else {
        tracing::info!(%thread, estimated, "Claude's auto checks stopped earlier; ask mode");
    }
}

/// The attach's UC-491, when this launch went to `default` over budget and the conversation was
/// not told yet.
pub(super) fn launch_notice(actor: &mut Actor, thread: ThreadId) -> Option<Notice> {
    let shown = shown(actor, thread, &NoticeKind::AskModeTooManyMatters);
    let state = state(actor, thread)?;
    let asking = state
        .agent
        .as_deref()
        .is_some_and(|agent| agent.auto == Auto::Ask);
    if !std::mem::take(&mut state.ask.launch_owed) || !asking || state.ask.told_launch || shown {
        return None;
    }
    state.ask.told_launch = true;
    Some(Notice {
        level: NoticeLevel::Info,
        text: TOO_MANY_MATTERS.to_owned(),
        kind: Some(NoticeKind::AskModeTooManyMatters),
    })
}

/// Whether `update` carries Claude Code's refusal for want of the auto-mode context: in a tool
/// call's content or output (inline, or a blob's preview), an answer or thought, or a notice.
/// Any tool's result counts, not only a failed one: a miss leaves every action refused, a false
/// match only makes Claude ask.
pub(crate) fn carries_refusal(update: &AgentUpdate) -> bool {
    match update {
        AgentUpdate::MessageChunk {
            role: MessageRole::Answer | MessageRole::Thought,
            content,
            ..
        } => block(content),
        AgentUpdate::ToolCall(call) => {
            call.content.iter().any(tool_content) || call.raw_output.as_ref().is_some_and(blob)
        }
        AgentUpdate::ToolCallUpdate { patch, .. } => {
            patch
                .content
                .as_ref()
                .is_some_and(|content| content.iter().any(tool_content))
                || patch.raw_output.as_ref().is_some_and(blob)
        }
        AgentUpdate::Notice { text, .. } => refusal(text),
        _ => false,
    }
}

/// `thread`'s Claude agent ran into the refusal ([`carries_refusal`]) outside a replay: once for
/// a session running in `auto`, the conversation keeps to ask mode, UC-492 is logged unless
/// either notice showed already, and the agent is marked to relaunch.
pub(crate) fn checks_stopped(actor: &mut Actor, thread: ThreadId) {
    let claude = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| AgentKind::of(&entry.row.agent) == Some(AgentKind::Claude));
    let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
    let told = shown(actor, thread, &NoticeKind::AskModeTooManyMatters)
        || shown(actor, thread, &NoticeKind::AskModeChecksStopped);
    let Some(state) = state(actor, thread) else {
        return;
    };
    let in_auto = state.agent.as_deref().is_some_and(|agent| {
        !agent.forced_ask && matches!(agent.auto, Auto::Wanted | Auto::Set { .. })
    });
    if !claude || !in_auto || state.ask.checks_stopped {
        return;
    }
    state.ask.checks_stopped = true;
    let notice = !(told || state.ask.told_launch || state.ask.told_stopped);
    state.ask.told_stopped |= notice;
    tracing::warn!(
        matter = ?matter,
        notice,
        "Claude Code refused a call for want of its auto checks; the conversation relaunches in \
         ask mode"
    );
    if notice {
        let notice = Notice {
            level: NoticeLevel::Warning,
            text: CHECKS_STOPPED.to_owned(),
            kind: Some(NoticeKind::AskModeChecksStopped),
        };
        if let Err(error) = commit(actor, thread, [notice.into()], Vec::new()) {
            tracing::warn!(%thread, error = %Cause(&error), "the ask-mode notice was not logged");
        }
    }
    mark_relaunch(actor, thread, Relaunch::ChecksStopped);
}

/// The mode a session held in ask mode that moved to `mode` goes back to, or `None` to keep it:
/// `default`, `acceptEdits` and `plan` are kept (none needs the auto-mode context); `auto` and
/// any other go back to `default`.
pub(crate) fn reset_to(mode: &str) -> Option<&'static str> {
    const KEPT: [&str; 3] = [
        crate::agent::forced_ask::ASK_MODE,
        crate::agent::loads::CLAUDE_FALLBACK_MODE,
        "plan",
    ];
    (!KEPT.contains(&mode)).then_some(crate::agent::forced_ask::ASK_MODE)
}

/// Whether the conversation's loaded view holds a host notice of `kind`.
fn shown(actor: &Actor, thread: ThreadId, kind: &NoticeKind) -> bool {
    actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .is_some_and(|view| {
            view.entries().values().any(|entry| {
                matches!(entry, ViewEntry::Notice(notice) if notice.kind.as_ref() == Some(kind))
            })
        })
}

fn tool_content(content: &ToolCallContent) -> bool {
    match content {
        ToolCallContent::Text { text } => blob(text),
        ToolCallContent::Content { content } => block(content),
        _ => false,
    }
}

fn block(content: &ContentBlock) -> bool {
    matches!(content, ContentBlock::Text { text } if refusal(text))
}

fn blob(text: &TextOrBlob) -> bool {
    match text {
        TextOrBlob::Inline(text) => refusal(text),
        TextOrBlob::Blob(blob) => refusal(blob.preview()),
    }
}

fn refusal(text: &str) -> bool {
    REFUSAL.iter().any(|part| text.contains(part))
}

#[cfg(test)]
#[path = "ask_mode_tests.rs"]
mod tests;
