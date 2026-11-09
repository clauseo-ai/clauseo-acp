//! The modes the host forces on a session: forced ask's (`agent/forced_ask.rs`, a Claude agent
//! in an unprotected folder) and a jailed Codex's (`agent/codex/jailed.rs`, R17 PR-D2c).
//!
//! - `SetMode` and `SetConfigOption` on the mode are gated before they reach the agent
//!   ([`refusal`]).
//! - Forced ask: a mode the agent moves to itself outside its set is logged as it came, then
//!   `session/set_mode` puts the session back to `default`, logged once it answers. Meanwhile
//!   forced ask's guard hook still asks before every tool, so the race costs no unasked change.
//! - A jailed Codex always runs the jailed preset; its mode is the host's policy, which the
//!   launch picked ([`launch_mode`]) and the agent records (`CodexAgent::jail`). The session's
//!   modes and options offer only `jailed::MODES`, at that mode ([`codex_start`], [`kept`]).
//!   Only the lawyer's commands change it ([`switch`]): one for the mode it runs in is answered
//!   at once; another is held to the turn boundary ([`at_boundary`]), where the agent is
//!   stopped and launched again under the new policy with the session resumed
//!   (`agents::restart_for_switch`), and its attach logs the new mode. The agent's own mode
//!   changes (`current_mode_update`, a mode option's value in `config_option_update`) are
//!   logged by kind and ignored ([`agent_moved`], [`kept`]): no `ModeChanged`, no relaunch.
//! - An unjailed Codex (tier C) runs `codex::ASK_MODE`: a start in another mode is switched
//!   there ([`codex_start`]); its own move to a mode that approves without asking is logged as
//!   it came and switched back, as forced ask's is, unless the lawyer sent it that mode
//!   ([`unjailed`]). The lawyer's own pick stands (D2e will gate it); a failed switch back
//!   stops the agent.

mod unjailed;

use std::sync::Arc;

use clauseo_acp::SessionStart;
use clauseo_protocol::{
    AgentChoiceId, CommandId, CommandOutcome, CompactString, ConfigCategory, ConfigOption,
    ConfigOptionKind, ConfigOptionsReplaced, ConfigValue, ErrorCode, HeaderEntry, ModeChanged,
    Notice, NoticeKind, NoticeLevel, StreamId, ThreadId,
};
use futures::future::{Either, select};

use super::super::Actor;
use super::agents::{Agent, Waiting, call};
use super::inputs::AgentReply;
use super::updates::{agent, commit};
use super::{answers, state};
use crate::agent::codex::{self, JAILED_MODE, jailed};
use crate::agent::{forced_ask, loads};
use crate::error::Error;

pub(super) use unjailed::CodexModes;
use unjailed::{Kind, capped};

/// `thread`'s agent, read only.
fn held(actor: &Actor, thread: ThreadId) -> Option<&Agent> {
    actor.threads.get(&thread)?.turn.agent.as_deref()
}

/// The mode (the policy) of `agent` when it is a Codex agent in the jail.
fn jail_of(agent: &Agent) -> Option<&'static str> {
    agent.codex.as_ref().and_then(|codex| codex.jail)
}

/// Why `waiting` may not reach `thread`'s agent, with the command it fails: a mode outside
/// forced ask's (`command.invalid`), or outside a jailed Codex's (`sandbox.ask_mode_unavailable`).
pub(super) fn refusal(
    actor: &Actor,
    thread: ThreadId,
    waiting: &Waiting,
) -> Option<(CommandId, Error)> {
    let agent = held(actor, thread)?;
    let jailed = jail_of(agent).is_some();
    let (ctx, mode) = match waiting {
        Waiting::Turn(_) => return None,
        Waiting::Mode(ctx, mode) => (ctx, Some(mode.as_str())),
        Waiting::Config(ctx, option, value) => {
            let mode = match value {
                ConfigValue::Select(choice) => Some(choice.as_str()),
                ConfigValue::Boolean(_) => None,
            };
            let sets_mode = if jailed {
                jailed::is_mode_option(option.as_str(), &known_options(actor, thread))
            } else {
                !forced_ask::config_allowed(option.as_str(), mode)
            };
            if !sets_mode {
                return None;
            }
            (ctx, mode)
        }
    };
    if agent.forced_ask && !mode.is_some_and(forced_ask::mode_allowed) {
        tracing::info!(mode = mode.unwrap_or("(none)"), "forced ask refused a mode");
        let error = Error::refused(
            ErrorCode::CommandInvalid,
            "That mode would approve changes without asking, and this matter's folder isn't \
             protected, so the mode stayed as it was.",
        );
        return Some((ctx.id, error));
    }
    if jailed {
        return jailed::check(mode.unwrap_or_default())
            .err()
            .map(|error| (ctx.id, error));
    }
    None
}

/// The thread's config options as last logged.
fn known_options(actor: &Actor, thread: ThreadId) -> Vec<ConfigOption> {
    header_of(actor, thread)
        .map(|header| header.config_options.clone())
        .unwrap_or_default()
}

/// The thread's header: from the loaded stream, else as the actor kept it at the last unload.
fn header_of(actor: &Actor, thread: ThreadId) -> Option<&HeaderEntry> {
    actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or(actor.threads.get(&thread)?.header.as_ref())
}

/// The mode `waiting` asks of a jailed Codex: `SetMode`'s, or a select value of its mode option.
fn asked_mode<'a>(actor: &Actor, thread: ThreadId, waiting: &'a Waiting) -> Option<&'a str> {
    match waiting {
        Waiting::Turn(_) | Waiting::Config(_, _, ConfigValue::Boolean(_)) => None,
        Waiting::Mode(_, mode) => Some(mode.as_str()),
        Waiting::Config(_, option, ConfigValue::Select(value)) => {
            jailed::is_mode_option(option.as_str(), &known_options(actor, thread))
                .then_some(value.as_str())
        }
    }
}

/// The mode, so the policy, of `thread`'s jailed Codex launch that `waiting` waits for: the last
/// switch to one of `jailed::MODES` among them, else the thread's saved mode as
/// `jailed::start_mode` reads it (an asking one is `jailed::SAVED_ASK_FALLBACK`).
pub(super) fn launch_mode(actor: &Actor, thread: ThreadId, waiting: &[Waiting]) -> &'static str {
    let asked = waiting
        .iter()
        .rev()
        .find_map(|item| asked_mode(actor, thread, item).filter(|mode| jailed::allowed(mode)));
    let saved = header_of(actor, thread).and_then(|header| header.mode_id.as_deref());
    jailed::launch_policy(asked, saved)
}

/// A Codex session's start: the mode the agent must run in, and whether it must be switched
/// there first. Tier C: [`codex::ASK_MODE`]. Jailed: always the preset, the start's modes and
/// options cut to `jailed::MODES`, the mode option at the launch's mode. A mode the agent does
/// not offer fails with `agent.protocol_error`.
pub(super) fn codex_start(
    actor: &mut Actor,
    thread: ThreadId,
    start: &mut SessionStart,
) -> Result<(AgentChoiceId, bool), Error> {
    let mode = if let Some(policy) = held(actor, thread).and_then(jail_of) {
        if let Some(modes) = &mut start.modes {
            jailed::keep_modes(modes);
        }
        jailed::keep_options(&mut start.config_options, Some(policy));
        JAILED_MODE
    } else {
        let current = start.modes.as_ref().map(|modes| modes.current.as_str());
        if current.is_some_and(codex::approves_unasked)
            && unjailed::first(actor, thread, Kind::Started)
        {
            tracing::info!(
                kind = Kind::Started.name(),
                "an unjailed Codex session started in a mode that approves without asking; \
                 switching it to the ask mode"
            );
        }
        codex::ASK_MODE
    };
    let offered = start
        .modes
        .as_ref()
        .is_some_and(|modes| modes.available.iter().any(|offered| offered.id == mode));
    if !offered {
        tracing::warn!(mode, "Codex did not offer the mode Clauseo runs it in");
        return Err(Error::refused(
            ErrorCode::AgentProtocolError,
            "Codex didn't offer the mode Clauseo runs it in, so no message was sent.",
        ));
    }
    let id = AgentChoiceId::new(mode).map_err(|error| Error::Internal(error.to_string()))?;
    let switch = start
        .modes
        .as_ref()
        .is_some_and(|modes| modes.current != mode);
    Ok((id, switch))
}

/// The mode `thread`'s attach logs in place of the agent's own: a jailed Codex's policy.
pub(super) fn shown(actor: &Actor, thread: ThreadId) -> Option<CompactString> {
    held(actor, thread)
        .and_then(jail_of)
        .map(CompactString::from)
}

/// The notice `thread`'s attach logs before its mode when a jailed Codex whose thread was last
/// in Read-only starts in the jailed preset's policy, where the jail does not enforce Read-only
/// (`jailed::read_only_unavailable`, Windows until PR6). Read before the attach logs the new
/// mode, so the thread is told once. Never for an unjailed Codex: it runs `codex::ASK_MODE`.
pub(super) fn read_only_notice(actor: &Actor, thread: ThreadId) -> Option<Notice> {
    let policy = held(actor, thread).and_then(jail_of)?;
    let saved = header_of(actor, thread).and_then(|header| header.mode_id.as_deref());
    if !jailed::read_only_unavailable(saved, policy) {
        return None;
    }
    tracing::info!(
        "a jailed Codex last in Read-only starts on its own: Read-only isn't enforced here"
    );
    Some(Notice {
        level: NoticeLevel::Info,
        text: jailed::READ_ONLY_UNAVAILABLE.to_owned(),
        kind: Some(NoticeKind::ReadOnlyUnavailable),
    })
}

/// The agent's config options for `thread`'s log: a jailed Codex's mode option keeps only
/// `jailed::MODES`, at the mode it runs in, whatever value the agent gave it.
pub(super) fn kept(
    actor: &mut Actor,
    thread: ThreadId,
    mut options: Vec<ConfigOption>,
) -> ConfigOptionsReplaced {
    if let Some(policy) = held(actor, thread).and_then(jail_of) {
        let moved = options.iter().any(|option| {
            let mode = option.id == jailed::MODE_OPTION || option.category == ConfigCategory::Mode;
            mode && matches!(&option.kind, ConfigOptionKind::Select { current_value, .. }
                if current_value != JAILED_MODE)
        });
        if moved {
            tracing::info!(
                kind = "config_option_update",
                "a jailed Codex changed its own mode option; ignored (only the lawyer changes it)"
            );
        }
        jailed::keep_options(&mut options, Some(policy));
    } else if let Some(mode) = option_mode(&options)
        && unjailed::moved(actor, thread, &mode, "config_option_update")
    {
        set_back(actor, thread, &mode, codex::ASK_MODE);
    }
    ConfigOptionsReplaced { options }
}

/// The current value of the mode option among `options`, if one holds a select value.
fn option_mode(options: &[ConfigOption]) -> Option<CompactString> {
    options.iter().find_map(|option| {
        let mode = option.id == jailed::MODE_OPTION || option.category == ConfigCategory::Mode;
        if let (true, ConfigOptionKind::Select { current_value, .. }) = (mode, &option.kind) {
            return Some(CompactString::from(current_value.as_str()));
        }
        None
    })
}

/// `mode` came from the agent (`current_mode_update`). A jailed Codex's is ignored, and its
/// session put back in the preset if it left it, unlogged; any other is logged, and switched
/// back when forced ask's set does not hold it, or when an unjailed Codex moved itself to a mode
/// that approves without asking which the lawyer did not send it (`unjailed::moved`).
pub(super) fn agent_moved(
    actor: &mut Actor,
    thread: ThreadId,
    mode: CompactString,
) -> Result<(), Error> {
    if held(actor, thread).and_then(jail_of).is_some() {
        tracing::info!(
            kind = "current_mode_update",
            "a jailed Codex changed its own mode; ignored (only the lawyer changes it)"
        );
        if mode != JAILED_MODE {
            set_back(actor, thread, &mode, JAILED_MODE);
        }
        return Ok(());
    }
    let forced = held(actor, thread).is_some_and(|agent| agent.forced_ask);
    if forced && !forced_ask::mode_allowed(&mode) {
        set_back(actor, thread, &mode, loads::CLAUDE_MODE);
    } else if unjailed::moved(actor, thread, &mode, "current_mode_update") {
        set_back(actor, thread, &mode, codex::ASK_MODE);
    }
    commit(
        actor,
        thread,
        [ModeChanged { mode_id: mode }.into()],
        Vec::new(),
    )
}

/// A lawyer's mode command for an unjailed Codex is about to reach it: `SetMode`'s mode, or a
/// select value of its mode option, becomes its pick (`unjailed::picked`).
fn note_pick(actor: &mut Actor, thread: ThreadId, waiting: &Waiting) {
    let picked = match waiting {
        Waiting::Turn(_) | Waiting::Config(_, _, ConfigValue::Boolean(_)) => return,
        Waiting::Mode(_, mode) => CompactString::from(mode.as_str()),
        Waiting::Config(_, option, ConfigValue::Select(value)) => {
            if !jailed::is_mode_option(option.as_str(), &known_options(actor, thread)) {
                return;
            }
            CompactString::from(value.as_str())
        }
    };
    let (Waiting::Mode(ctx, _) | Waiting::Config(ctx, ..)) = waiting else {
        return;
    };
    unjailed::picked(actor, thread, ctx.id, picked);
}

/// The lawyer's `SetMode` `command` for `mode` on `thread`'s agent answered, `ok` when it
/// succeeded (`unjailed::answered`).
pub(super) fn mode_answered(
    actor: &mut Actor,
    (thread, same): (ThreadId, bool),
    command: CommandId,
    mode: &str,
    ok: bool,
) {
    pick_answered(actor, (thread, same), command, ok.then_some(mode));
}

/// The lawyer's `SetConfigOption` `command` on `thread`'s agent answered with `options`, `None`
/// when it failed: the mode option's value is the mode it now runs (`unjailed::answered`).
pub(super) fn config_answered(
    actor: &mut Actor,
    (thread, same): (ThreadId, bool),
    command: CommandId,
    options: Option<&[ConfigOption]>,
) {
    let mode = options.and_then(option_mode);
    pick_answered(actor, (thread, same), command, mode.as_deref());
}

/// A lawyer's mode command answered (`unjailed::answered`): a pick that did not hold, whose
/// mode an unjailed Codex reported meanwhile, is switched back.
fn pick_answered(
    actor: &mut Actor,
    (thread, same): (ThreadId, bool),
    command: CommandId,
    mode: Option<&str>,
) {
    if !same {
        return;
    }
    if let Some(mode) = unjailed::answered(actor, thread, command, mode) {
        set_back(actor, thread, &mode, codex::ASK_MODE);
    }
}

/// `session/set_mode` to `target` for `thread`'s session, which moved itself to `mode`.
fn set_back(actor: &mut Actor, thread: ThreadId, mode: &str, target: &'static str) {
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    let (Some(session), Ok(id)) = (agent.session.clone(), AgentChoiceId::new(target)) else {
        return;
    };
    let (agent_id, client) = (agent.id, agent.link.client.clone());
    let unjailed = agent
        .codex
        .as_ref()
        .is_some_and(|codex| codex.jail.is_none());
    if unjailed {
        unjailed::resetting(actor, thread);
    }
    if unjailed::first(actor, thread, Kind::SetBack) {
        let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
        tracing::info!(
            kind = Kind::SetBack.name(),
            matter = ?matter,
            mode = %capped(mode),
            target,
            "a session moved to a mode outside the modes Clauseo allows it; switching it back"
        );
    }
    let clock = Arc::clone(&actor.clock);
    call(actor, thread, agent_id, async move {
        let answer = std::pin::pin!(client.set_mode(&session, id));
        let result = match select(answer, clock.after(unjailed::SET_BACK_WAIT)).await {
            Either::Left((result, _)) => result,
            Either::Right(((), _)) => Err(clauseo_acp::Error::Protocol(
                "no answer to session/set_mode in time".to_owned(),
            )),
        };
        AgentReply::ModeReset(CompactString::from(target), result)
    });
}

/// The switch back to `mode` answered: forced ask's and an unjailed Codex's are logged; a jailed
/// Codex's preset is not, since its mode is the policy's. An unjailed Codex's failure (an error,
/// or no answer in `unjailed::SET_BACK_WAIT`) stops it (`unjailed::stop_unasked`); one that
/// succeeded while the agent reported a mode that approves without asking is sent once more,
/// and a third stops it (`unjailed::After`). Any other failure is a warning (forced ask's hook
/// still asks; the jail holds whatever Codex's preset).
pub(super) fn on_mode_reset(
    actor: &mut Actor,
    thread: ThreadId,
    same: bool,
    mode_id: CompactString,
    result: Result<(), clauseo_acp::Error>,
) {
    if !same {
        return;
    }
    let jailed = held(actor, thread).and_then(jail_of).is_some();
    let again = match unjailed::reset_answered(actor, thread, result.is_ok()) {
        Some(unjailed::After::Stop) => {
            let error = result.err().map_or_else(
                || Error::Internal("it moved again while it was switched back".to_owned()),
                Error::Acp,
            );
            unjailed::stop_unasked(actor, thread, &error);
            return;
        }
        Some(unjailed::After::Again(mode)) => Some(mode),
        Some(unjailed::After::Done) | None => None,
    };
    let logged = result.map_err(Error::Acp).and_then(|()| {
        if jailed {
            return Ok(());
        }
        commit(actor, thread, [ModeChanged { mode_id }.into()], Vec::new())
    });
    if let Err(error) = logged {
        tracing::warn!(
            %thread,
            error = %crate::log_file::Cause(&error),
            "a session was not switched back to its allowed mode"
        );
    }
    if let Some(mode) = again {
        set_back(actor, thread, &mode, codex::ASK_MODE);
    }
}

/// A lawyer's `waiting` for `thread` on its attached agent: given back unless it asks a jailed
/// Codex for a mode (after [`refusal`], so one of `jailed::MODES`); an unjailed Codex's mode
/// becomes its pick on the way ([`note_pick`]). The mode it runs in is
/// answered now. Another becomes the pending switch (a later one replaces it, answered with no
/// event, its receipt `Replaced`): the agent relaunches under its policy now when no turn runs,
/// else at the boundary.
pub(super) fn switch(actor: &mut Actor, thread: ThreadId, waiting: Waiting) -> Option<Waiting> {
    let Some(policy) = held(actor, thread).and_then(jail_of) else {
        note_pick(actor, thread, &waiting);
        return Some(waiting);
    };
    let Some(asked) = asked_mode(actor, thread, &waiting) else {
        return Some(waiting);
    };
    let asked = if asked == jailed::READ_ONLY {
        jailed::READ_ONLY
    } else {
        JAILED_MODE
    };
    let Some(state) = state(actor, thread) else {
        return Some(waiting);
    };
    let running = state.current.is_some() || state.closing.is_some();
    let Some(agent) = state.agent.as_deref_mut() else {
        return Some(waiting);
    };
    let replaced = agent.switch.take();
    let now = if asked == policy {
        Some(waiting)
    } else {
        tracing::info!(
            mode = asked,
            running,
            "a jailed Codex's mode switches by a relaunch"
        );
        agent.switch = Some(waiting);
        None
    };
    if let Some(old) = replaced {
        answer(actor, thread, &old, CommandOutcome::Replaced);
    }
    match now {
        // Its attach logged the mode it runs in, and nothing else logs a jailed Codex's mode.
        Some(waiting) => answer(actor, thread, &waiting, CommandOutcome::Done),
        None if !running => super::agents::restart_for_switch(actor, thread),
        None => {}
    }
    None
}

/// The work `thread`'s jailed Codex runs once attached, its mode commands but the last one
/// answered as replaced (as [`switch`] answers one it replaces): the last chose the launch's
/// policy ([`launch_mode`]), so replaying an older one would relaunch it, and the relaunch
/// would get the same queue again.
pub(super) fn latest_mode(
    actor: &mut Actor,
    thread: ThreadId,
    waiting: Vec<Waiting>,
) -> Vec<Waiting> {
    if held(actor, thread).and_then(jail_of).is_none() {
        return waiting;
    }
    let modes: Vec<bool> = waiting
        .iter()
        .map(|item| asked_mode(actor, thread, item).is_some_and(jailed::allowed))
        .collect();
    let Some(last) = modes.iter().rposition(|mode| *mode) else {
        return waiting;
    };
    let mut kept = Vec::with_capacity(waiting.len());
    for ((at, item), mode) in waiting.into_iter().enumerate().zip(modes) {
        if mode && at != last {
            answer(actor, thread, &item, CommandOutcome::Replaced);
        } else {
            kept.push(item);
        }
    }
    kept
}

/// Answers a jailed Codex's mode command `waiting` with `outcome` and no `ModeChanged`: the
/// attach of the agent that runs its mode logged it. `Replaced` for one a later command
/// replaced, so no app reads it as its mode being set. A `SetConfigOption` gets the thread's
/// options, the mode option at the mode run in.
fn answer(actor: &mut Actor, thread: ThreadId, waiting: &Waiting, outcome: CommandOutcome) {
    let options = matches!(waiting, Waiting::Config(..)).then(|| {
        let options = known_options(actor, thread);
        kept(actor, thread, options)
    });
    match waiting {
        Waiting::Mode(ctx, _) | Waiting::Config(ctx, ..) => {
            answers::jail_mode(actor, thread, (*ctx, outcome), options);
        }
        Waiting::Turn(_) => {}
    }
}

/// `thread`'s turn ended (durable): a pending switch relaunches its agent now, before the queue
/// moves on, so the next turn runs under the new policy.
pub(super) fn at_boundary(actor: &mut Actor, thread: ThreadId) {
    unjailed::flush(actor, thread);
    if held(actor, thread).is_some_and(|agent| agent.switch.is_some()) {
        super::agents::restart_for_switch(actor, thread);
    }
}
