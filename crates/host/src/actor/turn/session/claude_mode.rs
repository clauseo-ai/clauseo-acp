//! A Claude session's mode at its start: Clauseo's (`agent::loads::claude_mode`: `auto`, or
//! `default` in forced ask), else, outside forced ask, `acceptEdits`
//! (`agent::loads::CLAUDE_FALLBACK_MODE`) when auto is not available, with one notice for the
//! lawyer per thread and host run ([`notice`]).
//!
//! claude-agent-acp 0.89.1 starts a session in the matter's own `permissions.defaultMode`
//! (`<matter>/.claude/settings.local.json` and the like, or a resumed transcript's mode):
//! `resolveInitialPermissionMode` reads it from `resolveSettings({cwd})`, which has no flag
//! settings (`session-mode.js:22-35`, `settings.js:88-95`), and `_meta` cannot override it,
//! since the adapter's table of managed options marks `permissionMode` as not settable
//! (`acp-agent.js:388`). So `session/new`, `load` or `resume` may answer `acceptEdits`, `plan`,
//! `bypassPermissions` or any other mode, and the host's `session/set_mode` goes first, before
//! any prompt ([`claude_start`], `session.rs` `prepare`; no prompt is sent before the attach).
//! A session that already reports the mode wanted needs no switch.
//!
//! The switch is read back ([`prepared`]): an empty answer proves nothing, so the last mode the
//! agent reported among the updates it sent after the switch went out (held until the attach,
//! `forced_mode/read_back.rs`) must be the one asked for. Auto asked and `acceptEdits`
//! reported is the fallback below; any other mode, or none reported, refuses the launch with
//! `agent.mode_not_set` before any prompt.
//!
//! Auto is not available when:
//!
//! - the session's `availableModes` lacks `auto` ([`claude_start`]);
//! - `session/set_mode auto` answers an error ([`switch`]): Claude Code refuses auto when the
//!   account's plan, the settings or fast mode rule it out ("Cannot set permission mode to auto:
//!   …", CLI 2.1.293), and claude-agent-acp 0.89.1 passes that back as the RPC error
//!   (`session-mode.js:143-152`);
//! - the read-back is `acceptEdits`: claude-agent-acp 0.89.1 itself moves a session whose model
//!   cannot run auto to `acceptEdits` and says so (`session-mode.js:98-102`, `:161-162`).
//!
//! claude-agent-acp 0.89.1 lists `auto` whatever the account (`session-mode.js:233-258`), and a
//! CLI that drops to `default` on its own (its remote kill switch) is never relayed, so the host
//! cannot see that case.

use clauseo_acp::{AcpClient, AcpEvent, AgentUpdate, SessionStart};
use clauseo_protocol::{
    AgentChoiceId, CompactString, ErrorCode, Notice, NoticeKind, NoticeLevel, ThreadId,
};

use super::super::super::Actor;
use super::super::agents::launch_failed;
use super::super::forced_mode::read_back;
use super::super::picks::Pick;
use super::super::state;
use super::{attach, before_attach};
use crate::agent::loads::{CLAUDE_FALLBACK_MODE, CLAUDE_MODE, claude_mode};
use crate::error::Error;
use crate::log_file::Cause;

/// The host English of the notice a thread gets when Claude's `auto` mode is not available
/// (`NoticeKind::AutoModeUnavailable`), for a view that does not know the kind. Provisional: the
/// session runs in `acceptEdits`, where file changes go unasked and, with the sandbox, so do
/// sandboxed commands (`pinned.rs`, `autoAllowBashIfSandboxed`), so it never says Claude asks
/// before running commands; and the cause may be the model, not the account.
/// The app's own line (`notice-words.ts`, TN.json) awaits the app's words.
pub(crate) const AUTO_UNAVAILABLE: &str = "Claude's automatic mode isn't available here, so \
     Claude makes file changes without asking and asks before steps outside its protection.";

/// Where a Claude agent stands with `auto`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) enum Auto {
    /// Not switched by the host, or switched and kept: `auto` is the mode it goes back to.
    #[default]
    Wanted,
    /// `session/set_mode auto` went out before the attach; its read-back is due ([`prepared`]).
    Set,
    /// Not available: the session runs in `acceptEdits` and goes back to it. (A launch whose
    /// sandbox can't be built on macOS or Linux never gets here: it is refused,
    /// `agent::loads::sandbox::Off::refusal`.)
    Unavailable,
    /// As `Wanted`, with a `SetConfigOption` of the model out (`model_switch.rs`): `from` is
    /// the model option and the value it had, and `left` whether the agent has since moved the
    /// session to `acceptEdits` (claude-agent-acp 0.89.1 does when the new model cannot run
    /// auto, `session-mode.js:122-134`, before it answers).
    Switching { from: Pick, left: bool },
}

impl Auto {
    /// Whether the session runs in `acceptEdits` instead of `auto`.
    pub(crate) fn off(&self) -> bool {
        *self == Self::Unavailable
    }
}

/// A Claude agent's session started: when it is not in its mode (`auto`, or `default` in forced
/// ask), `session/set_mode` goes first, before any prompt, through `prepare`, since
/// claude-agent-acp 0.89.1 starts it in the user's own `permissions.defaultMode` (or a resumed
/// transcript's mode) and `_meta` cannot override that (see `agent/loads.rs`). Outside forced
/// ask a session that offers no `auto` runs in `acceptEdits` instead, and a refused switch to
/// `auto` falls back to it (`session.rs` `checks`). Any other failed switch fails the launch, so
/// no prompt runs in the user's mode. One that reports no modes, or offers no mode the host may
/// run it in, fails the launch ([`no_mode`]). On load and resume this also resets a mode the
/// lawyer reached in the session.
pub(super) fn claude_start(
    actor: &mut Actor,
    thread: ThreadId,
    (start, loaded): (SessionStart, bool),
    picks: Vec<Pick>,
) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    let forced_ask = agent.forced_ask;
    let wanted = claude_mode(forced_ask);
    let Some(modes) = start.modes.as_ref() else {
        return no_mode(actor, thread, wanted, "reported no modes");
    };
    let offered = |id: &str| modes.available.iter().any(|mode| mode.id == id);
    let target = if !forced_ask && (agent.auto == Auto::Unavailable || !offered(wanted)) {
        if agent.auto != Auto::Unavailable {
            tracing::warn!(%thread, "Claude offered no auto mode; it runs in acceptEdits");
        }
        agent.auto = Auto::Unavailable;
        CLAUDE_FALLBACK_MODE
    } else {
        wanted
    };
    if modes.current == target {
        if picks.is_empty() {
            return attach(actor, thread, start, loaded);
        }
        return before_attach(actor, thread, (start, loaded), (None, None, picks));
    }
    let (true, Ok(mode)) = (offered(target), AgentChoiceId::new(target)) else {
        return no_mode(actor, thread, target, "offered no such mode");
    };
    let fallback = (target == CLAUDE_MODE && offered(CLAUDE_FALLBACK_MODE))
        .then(|| AgentChoiceId::new(CLAUDE_FALLBACK_MODE).ok())
        .flatten();
    if target == CLAUDE_MODE {
        agent.auto = Auto::Set;
    }
    tracing::info!(%thread, from = %modes.current, to = target, "switching the Claude session to Clauseo's mode");
    let held = Some(agent.replayed.len());
    let switch = Switch {
        mode,
        fallback,
        held,
    };
    before_attach(actor, thread, (start, loaded), (Some(switch), None, picks));
}

/// Fails the launch of a Claude session whose mode the host cannot set to `mode`: `why` says
/// what the agent's answer lacked.
fn no_mode(actor: &mut Actor, thread: ThreadId, mode: &str, why: &str) {
    let error = Error::refused(
        ErrorCode::AgentProtocolError,
        format!(
            "Claude Code {why}, so Clauseo cannot run it in its \"{mode}\" mode and sent no message"
        ),
    );
    tracing::warn!(
        %thread,
        error = %Cause(&error),
        "the Claude session cannot run in Clauseo's mode"
    );
    launch_failed(actor, thread, &error);
}

/// A `session/set_mode` before the attach: to `mode`, and, should the agent refuse it, to
/// `fallback` when there is one (Claude's `acceptEdits` for `auto`), else the launch fails.
/// `held` is how many updates the agent had held as it went out, where a Claude switch's
/// read-back starts ([`prepared`]); `None` for Codex's ask mode, which is not read back.
#[derive(Debug)]
pub(super) struct Switch {
    pub(super) mode: AgentChoiceId,
    pub(super) fallback: Option<AgentChoiceId>,
    pub(super) held: Option<usize>,
}

/// What a Claude agent's [`Switch`] did, for its read-back in [`prepared`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Switched {
    /// Where the read-back starts in the agent's held updates.
    held: usize,
    /// The mode last asked for: the switch's, or its fallback's.
    asked: CompactString,
    /// Whether a `session/set_mode` to `asked` went out: not when the switch fell back to the
    /// mode the session's start already reported.
    sent: bool,
    /// Whether the switch to `auto` was refused and fell back.
    fell_back: bool,
}

/// `prepare`'s mode switch (`session.rs` `checks`): `session/set_mode` to the switch's mode, or
/// to its fallback when the agent refuses that (no call when the session already reported the
/// fallback). The start's mode becomes the one asked for; [`prepared`] reads it back.
pub(super) async fn switch(
    client: &AcpClient,
    start: &mut SessionStart,
    Switch {
        mode,
        fallback,
        held,
    }: Switch,
) -> Result<Option<Switched>, Error> {
    let (asked, sent, fell_back) = match (
        client.set_mode(&start.session, mode.clone()).await,
        fallback,
    ) {
        (Ok(()), _) => (mode, true, false),
        (Err(error), None) => return Err(error.into()),
        (Err(error), Some(fallback)) => {
            tracing::warn!(error = %Cause(&error), to = %fallback.as_str(), "the agent refused its mode; falling back");
            let current = start.modes.as_ref().map(|modes| modes.current.as_str());
            let sent = current != Some(fallback.as_str());
            if sent {
                client.set_mode(&start.session, fallback.clone()).await?;
            }
            (fallback, sent, true)
        }
    };
    let asked = CompactString::from(asked.as_str());
    if let Some(modes) = &mut start.modes {
        modes.current.clone_from(&asked);
    }
    Ok(held.map(|held| Switched {
        held,
        asked,
        sent,
        fell_back,
    }))
}

/// `prepare` ended well: a Claude agent's switch (`switched`) is read back from the updates
/// it held since the switch went out (`read_back::last_reported`; none reported is the mode the
/// start had when no call went out). The mode asked for keeps the launch going: `auto` stays
/// wanted, and a fallback leaves auto unavailable. Auto asked and `acceptEdits` reported leaves
/// auto unavailable too. Anything else fails the launch with `agent.mode_not_set` before any
/// prompt, and `false` says so. The start's mode becomes the one reported, which the attach
/// logs, and the held `current_mode_update`s since the switch go.
pub(super) fn prepared(
    actor: &mut Actor,
    thread: ThreadId,
    switched: Option<Switched>,
    start: &mut SessionStart,
) -> bool {
    let Some(switched) = switched else {
        return true;
    };
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return false;
    };
    let held = agent.replayed.get(switched.held..).unwrap_or_default();
    let reported = read_back::last_reported(held).map(CompactString::from);
    let wanted = switched.asked.as_str();
    let now = reported
        .clone()
        .or_else(|| (!switched.sent).then(|| switched.asked.clone()));
    match now.as_deref() {
        Some(mode) if mode == wanted && switched.fell_back => {
            tracing::warn!(%thread, "Claude's auto mode is not available; it runs in acceptEdits");
            agent.auto = Auto::Unavailable;
        }
        Some(mode) if mode == wanted && wanted == CLAUDE_MODE => agent.auto = Auto::Wanted,
        Some(mode) if mode == wanted => {}
        Some(CLAUDE_FALLBACK_MODE) if wanted == CLAUDE_MODE => {
            tracing::warn!(%thread, "Claude moved itself to acceptEdits; auto is not available");
            agent.auto = Auto::Unavailable;
        }
        _ => {
            let error = read_back::not_set(thread, wanted, reported.as_deref());
            launch_failed(actor, thread, &error);
            return false;
        }
    }
    // The attach logs the mode read back; the moves that led there go.
    let mut index = 0;
    agent.replayed.retain(|event| {
        index += 1;
        index <= switched.held
            || !matches!(
                event,
                AcpEvent::Update {
                    update: AgentUpdate::CurrentMode { .. },
                    ..
                }
            )
    });
    if let (Some(now), Some(modes)) = (now, &mut start.modes) {
        modes.current = now;
    }
    true
}

/// The notice that auto is not available, for the attach commit: once per thread for the host's
/// run, and only while the agent runs without auto.
pub(super) fn notice(actor: &mut Actor, thread: ThreadId) -> Option<Notice> {
    let state = state(actor, thread)?;
    let unavailable = state
        .agent
        .as_deref()
        .is_some_and(|agent| agent.auto == Auto::Unavailable);
    if !unavailable || state.auto_told {
        return None;
    }
    state.auto_told = true;
    Some(Notice {
        level: NoticeLevel::Info,
        text: AUTO_UNAVAILABLE.to_owned(),
        kind: Some(NoticeKind::AutoModeUnavailable),
    })
}
