//! Ask mode for a Claude conversation whose auto-mode checks cannot work: the session runs in
//! `default` (`forced_ask::ASK_MODE`, Claude asks before each change) instead of `auto`, with
//! every deny rule still in its settings, so Claude Code checks them locally and no server
//! context is needed. Nothing is trimmed and no launch is refused. Two ways in:
//!
//! - At launch ([`at_launch`]), on the final rule set (after `SHED_AT`'s shedding) and before
//!   the session call: the launch's flag settings would give an auto-mode context over
//!   `agent::auto_context::ASK_MODE_AT` (Claude Code 2.1.293 sends none above 1,040,000 bytes,
//!   and then refuses every `Edit` and `Bash` of the launch), or the data folder's shell rules
//!   were shed to fit the settings file (`agent::loads::profile::SHED_AT`), whatever the
//!   estimate. Logged at warn; the notice `NoticeKind::AskModeTooManyMatters` (UC-491) goes with
//!   the attach ([`launch_notice`]).
//! - Detected ([`checks_stopped`]), from what a session running in `auto` sends ([`seen`], keyed
//!   on the tool result's text): Claude Code's refusal because the request carried no auto-mode
//!   context (`Seen::Withheld`), as when the limit moves. Logged at warn, the notice
//!   `NoticeKind::AskModeChecksStopped` (UC-492) is logged, and the agent is marked to relaunch
//!   (`idle::Relaunch::ChecksStopped`): its next prompt stops it and starts it again, which
//!   resumes the session in `default`. From then on every launch of the conversation runs in
//!   `default` for the host's run (the thread's turn state keeps it); a host started again
//!   decides from the estimate once more. A session already in `default` never triggers it, and a
//!   replayed transcript is not read, so the relaunch cannot loop.
//!
//! A service blip is not a reason for ask mode (`Seen::Blip`: any other "gave no verdict", such
//! as "(error)", which a tiny rule list gets too). Claude Code retries a transient one itself
//! (its text says to, with a growing delay, and stops the turn after 10 in a row), so the host
//! only watches: a later tool call of the turn that completes clears it; a turn that ends on a
//! blip ([`turn_ending`]) gets one plain notice, `NoticeKind::SafetyCheckNoAnswer` (UC-800),
//! logged at warn, and no mode change, unless the launch's estimate is at least
//! `agent::auto_context::ASK_ON_BLIP_AT` (80% of the limit), where it goes to ask mode as above.
//!
//! Each ask-mode notice shows at most once per conversation: neither again once the conversation
//! holds it (its loaded view) or the host told it this run; and UC-492 not at all once UC-491
//! showed. UC-800 shows at most once a turn. Ask mode wins over `acceptEdits` for want of a
//! sandbox (`Auto::NoSandbox`): `default` asks for more.

use clauseo_protocol::{AgentKind, Notice, NoticeKind, NoticeLevel, StreamId, ThreadId, ViewEntry};
use serde_json::{Map, Value};

use super::super::super::Actor;
use super::super::idle::{Relaunch, mark_relaunch};
use super::super::state;
use super::super::updates::commit;
use super::Auto;
use crate::agent::auto_context::{ASK_MODE_AT, ASK_ON_BLIP_AT, estimate, settings_of};
use crate::log_file::Cause;

mod seen;
pub(crate) use seen::{Seen, seen};

/// The host English of UC-491 (`NoticeKind::AskModeTooManyMatters`), for a view that does not
/// know the kind.
pub(crate) const TOO_MANY_MATTERS: &str = "Claude will ask before each change in this \
     conversation. Clauseo is protecting more matters than Claude's automatic checks can handle \
     at once, so Claude asks you instead.";

/// The host English of UC-492 (`NoticeKind::AskModeChecksStopped`).
pub(crate) const CHECKS_STOPPED: &str = "Claude now asks before each change in this \
     conversation, because its automatic checks stopped working here. Clauseo's own \
     protections still apply.";

/// The host English of UC-800 (`NoticeKind::SafetyCheckNoAnswer`).
pub(crate) const NO_ANSWER: &str = "Claude's safety check didn't answer, so Claude didn't do \
     this. Try again in a moment.";

/// A conversation's ask mode, kept with its turn state across launches for the host's run.
#[derive(Debug, Default)]
pub(crate) struct AskMode {
    /// A session of it ran into the refusal: every later launch runs in `default`.
    checks_stopped: bool,
    /// This launch went to `default` over budget; its attach owes UC-491.
    launch_owed: bool,
    /// A blip no later completed tool call cleared; the turn's end reads it.
    blip: bool,
    /// The last Claude launch's estimate.
    estimated: usize,
    /// What the host told the conversation this run.
    told: Told,
}

/// The notices told this run.
#[derive(Debug, Default)]
struct Told {
    /// UC-491.
    launch: bool,
    /// UC-492.
    stopped: bool,
    /// That Claude's auto mode is not available (`claude_mode.rs`).
    auto: bool,
}

impl AskMode {
    /// Marks the auto-unavailable notice told (`claude_mode::notice`, once per thread for the
    /// host's run); whether it was not told yet.
    pub(super) fn first_auto_notice(&mut self) -> bool {
        !std::mem::replace(&mut self.told.auto, true)
    }
}

/// Whether a launch goes to ask mode for its rules: its estimate (`estimated`) over
/// [`ASK_MODE_AT`], or its data folder's shell rules `shed` to fit the settings file.
pub(crate) fn too_many_rules(estimated: usize, shed: bool) -> bool {
    estimated > ASK_MODE_AT || shed
}

/// Before `thread`'s session call, with its `_meta` (`meta`) built and its rules final
/// (`shed`: the data folder's shell rules were shed to fit the settings file): holds a Claude
/// launch in `default` ([`Auto::Ask`]) when [`too_many_rules`] says, or when the
/// conversation's checks stopped earlier this run. Forced ask (already `default`) is left as it
/// is; another agent's launch too.
pub(crate) fn at_launch(
    actor: &mut Actor,
    thread: ThreadId,
    (kind, shed): (Option<AgentKind>, bool),
    meta: &Map<String, Value>,
) {
    if kind != Some(AgentKind::Claude) {
        return;
    }
    let estimated = settings_of(meta).map_or(0, estimate);
    let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
    let Some(state) = state(actor, thread) else {
        return;
    };
    let over = too_many_rules(estimated, shed);
    tracing::info!(%thread, estimated, shed, "Claude's auto-mode context estimated");
    state.ask.estimated = estimated;
    state.ask.blip = false;
    let Some(agent) = state.agent.as_deref_mut() else {
        return;
    };
    if agent.forced_ask || !(over || state.ask.checks_stopped) {
        return;
    }
    agent.auto = Auto::Ask;
    if over {
        tracing::warn!(
            matter = ?matter,
            estimated,
            shed,
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
    if !std::mem::take(&mut state.ask.launch_owed) || !asking || state.ask.told.launch || shown {
        return None;
    }
    state.ask.told.launch = true;
    Some(Notice {
        level: NoticeLevel::Info,
        text: TOO_MANY_MATTERS.to_owned(),
        kind: Some(NoticeKind::AskModeTooManyMatters),
    })
}

/// What `thread`'s agent sent outside a replay, as [`seen`] read it: a withheld refusal goes
/// to ask mode ([`checks_stopped`]); a blip waits for the turn's end, unless a later tool call
/// completes. Only for a Claude session running in `auto`.
pub(crate) fn on_seen(actor: &mut Actor, thread: ThreadId, seen: Seen) {
    if seen == Seen::Withheld {
        return checks_stopped(actor, thread);
    }
    let in_auto = in_auto(actor, thread);
    let Some(state) = state(actor, thread) else {
        return;
    };
    match seen {
        Seen::Blip if in_auto => {
            if !std::mem::replace(&mut state.ask.blip, true) {
                tracing::info!(%thread, "Claude's auto checks gave no verdict; Claude may retry");
            }
        }
        Seen::Done => state.ask.blip = false,
        Seen::Blip | Seen::Withheld => {}
    }
}

/// `thread`'s turn is ending: when it ends on a blip, UC-800 once (logged at warn), or ask
/// mode when the launch's estimate is at least [`ASK_ON_BLIP_AT`].
pub(crate) fn turn_ending(actor: &mut Actor, thread: ThreadId) {
    let in_auto = in_auto(actor, thread);
    let Some(state) = state(actor, thread) else {
        return;
    };
    let estimated = state.ask.estimated;
    if !std::mem::take(&mut state.ask.blip) || !in_auto {
        return;
    }
    if estimated >= ASK_ON_BLIP_AT {
        return checks_stopped(actor, thread);
    }
    let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
    tracing::warn!(matter = ?matter, estimated, "Claude's turn ended on a blip of its auto checks");
    let notice = Notice {
        level: NoticeLevel::Warning,
        text: NO_ANSWER.to_owned(),
        kind: Some(NoticeKind::SafetyCheckNoAnswer),
    };
    if let Err(error) = commit(actor, thread, [notice.into()], Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "the safety-check notice was not logged");
    }
}

/// Whether `thread`'s agent is a Claude session running in `auto`, outside forced ask.
fn in_auto(actor: &mut Actor, thread: ThreadId) -> bool {
    let claude = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| AgentKind::of(&entry.row.agent) == Some(AgentKind::Claude));
    claude
        && state(actor, thread)
            .and_then(|state| state.agent.as_deref())
            .is_some_and(|agent| {
                !agent.forced_ask && matches!(agent.auto, Auto::Wanted | Auto::Set { .. })
            })
}

/// `thread`'s Claude agent ran into a withheld refusal, or ended a turn on a blip at a large
/// estimate: once for a session running in `auto`, the conversation keeps to ask mode, UC-492 is
/// logged unless either notice showed already, and the agent is marked to relaunch.
pub(crate) fn checks_stopped(actor: &mut Actor, thread: ThreadId) {
    let in_auto = in_auto(actor, thread);
    let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
    let told = shown(actor, thread, &NoticeKind::AskModeTooManyMatters)
        || shown(actor, thread, &NoticeKind::AskModeChecksStopped);
    let Some(state) = state(actor, thread) else {
        return;
    };
    if !in_auto || state.ask.checks_stopped {
        return;
    }
    state.ask.checks_stopped = true;
    let notice = !(told || state.ask.told.launch || state.ask.told.stopped);
    state.ask.told.stopped |= notice;
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

#[cfg(test)]
#[path = "ask_mode_tests.rs"]
mod tests;
