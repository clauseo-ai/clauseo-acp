//! Codex's automatic mode, refused by the account. A Codex session runs in `codex::MODE`, whose
//! turns ask for Codex's reviewer; an account whose requirements forbid that reviewer refuses
//! each prompt before its turn starts (`clauseo_acp::Error::auto_review_refused`), and codex-acp
//! 2.2.2 takes `session/set_mode agent` without a check (`dist/index.js:43644-43710`), so the
//! prompt is where the refusal shows. Then, as Claude falls back to `acceptEdits` when `auto` is
//! refused (`session/claude_mode.rs`):
//!
//! - the prompt's call sets the session to `codex::FALLBACK_MODE` and sends the same prompt
//!   once more ([`Resend`]); a second failure, the resend's or the switch's, is the turn's as
//!   any other, and a Stop meanwhile sends nothing again;
//! - the agent is remembered for the host's run (`Actor::codex_reviewer_refused`), so its later
//!   sessions start in the fallback mode (`session.rs`) and pay no refused turn;
//! - each thread that runs in the fallback mode gets one notice, once for the host's run
//!   ([`notice`]; `auto_told`, Claude's flag), with `NoticeKind::AutoModeUnavailable`.
//!
//! Any other error passes through unchanged.

use std::rc::Rc;

use clauseo_acp::{AcpClient, PromptEnd};
use clauseo_protocol::{
    AgentChoiceId, AgentKind, AgentSessionId, CompactString, DomainEvent, ModeChanged, Notice,
    NoticeKind, NoticeLevel, ThreadId,
};

use super::super::Actor;
use super::key_retry::Stopped;
use super::{AgentReply, state};
use crate::agent::codex;
use crate::log_file::Cause;

/// The host English of the notice a thread gets when Codex's automatic mode is not available
/// (`NoticeKind::AutoModeUnavailable` from a Codex agent), for a view that does not know the
/// kind.
pub(crate) const CODEX_AUTO_UNAVAILABLE: &str = "ChatGPT's automatic mode isn't available for \
     this account, so ChatGPT asks before approving steps.";

/// A prompt's one resend in the fallback mode, for a Codex agent only.
#[derive(Debug, Default)]
pub(super) struct Resend {
    /// The turn's Stop, for a Codex agent; `None` for any other, which never resends.
    stopped: Option<Stopped>,
}

impl Resend {
    /// The resend for a prompt of a Codex agent (`codex`) whose turn's Stop sets `stopped`.
    pub(super) fn for_prompt(codex: bool, stopped: &Stopped) -> Self {
        Self {
            stopped: codex.then(|| Rc::clone(stopped)),
        }
    }

    /// `first` is the prompt's answer. When it is the reviewer's refusal: `session/set_mode` to
    /// the fallback mode, then `send` once more unless the turn was stopped. The flag says the
    /// session is in the fallback mode now; a failed switch keeps `first`, flag unset.
    pub(super) async fn run<F, Fut>(
        &self,
        client: &AcpClient,
        session: &AgentSessionId,
        first: Result<PromptEnd, clauseo_acp::Error>,
        send: F,
    ) -> (Result<PromptEnd, clauseo_acp::Error>, bool)
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<PromptEnd, clauseo_acp::Error>>,
    {
        let Some(stopped) = &self.stopped else {
            return (first, false);
        };
        if !matches!(&first, Err(error) if error.auto_review_refused()) {
            return (first, false);
        }
        let Ok(fallback) = AgentChoiceId::new(codex::FALLBACK_MODE) else {
            return (first, false);
        };
        tracing::warn!(
            to = codex::FALLBACK_MODE,
            "the account refused Codex's reviewer; falling back and sending the prompt again"
        );
        if let Err(error) = client.set_mode(session, fallback).await {
            tracing::warn!(error = %Cause(&error), "the Codex session was not switched to its fallback");
            return (first, false);
        }
        if stopped.get() {
            tracing::info!("the turn was stopped during the switch; the prompt is not sent again");
            return (first, true);
        }
        (send().await, true)
    }
}

/// The mode `thread`'s Codex session runs in: the fallback once its agent's account refused the
/// reviewer this host run, else [`codex::MODE`].
pub(super) fn mode(actor: &Actor, thread: ThreadId) -> &'static str {
    let refused = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| actor.codex_reviewer_refused.contains(&entry.row.agent));
    if refused {
        codex::FALLBACK_MODE
    } else {
        codex::MODE
    }
}

/// A reply of `thread`'s agent (`same`: of its current one): a prompt that fell back goes to
/// [`fell_back`].
pub(super) fn on_reply(actor: &mut Actor, thread: ThreadId, same: bool, reply: &AgentReply) {
    if same
        && matches!(
            reply,
            AgentReply::Prompt {
                fell_back: true,
                ..
            }
        )
    {
        fell_back(actor, thread);
    }
}

/// `thread`'s prompt fell back: its agent is remembered, and the mode and the notice (when the
/// thread has not had it) are logged.
fn fell_back(actor: &mut Actor, thread: ThreadId) {
    let Some(agent) = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone())
    else {
        return;
    };
    tracing::info!(%thread, "Codex's automatic mode is not available; it runs in workspace-write");
    actor.codex_reviewer_refused.insert(agent);
    let mut events: Vec<DomainEvent> = vec![
        ModeChanged {
            mode_id: CompactString::from(codex::FALLBACK_MODE),
        }
        .into(),
    ];
    events.extend(notice(actor, thread).map(Into::into));
    if let Err(error) = super::updates::commit(actor, thread, events, Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "the Codex fallback was not logged");
    }
}

/// The notice that Codex's automatic mode is not available, once per thread for the host's run,
/// for a Codex thread whose agent's account refused the reviewer.
pub(super) fn notice(actor: &mut Actor, thread: ThreadId) -> Option<Notice> {
    let entry = actor.threads.get(&thread)?;
    let refused = AgentKind::of(&entry.row.agent) == Some(AgentKind::Codex)
        && actor.codex_reviewer_refused.contains(&entry.row.agent);
    let state = state(actor, thread)?;
    if !refused || state.auto_told {
        return None;
    }
    state.auto_told = true;
    Some(Notice {
        level: NoticeLevel::Info,
        text: CODEX_AUTO_UNAVAILABLE.to_owned(),
        kind: Some(NoticeKind::AutoModeUnavailable),
    })
}
