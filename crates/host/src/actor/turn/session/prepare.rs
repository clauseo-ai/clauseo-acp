//! A session's work between its start's answer and its attach (`session.rs`): the mode
//! switch, Codex's guard check and the model and effort picks, held updates meanwhile; then, for
//! picks made while it ran, another round (`picks::late`), else the attach.

use std::path::PathBuf;

use clauseo_acp::{AcpClient, SessionStart};
use clauseo_protocol::{AgentChoiceId, CompactString, ThreadId};

use super::super::super::Actor;
use super::super::agents::call;
use super::super::picks::{Pick, Picked};
use super::super::{AgentReply, state};
use super::{attach, claude_mode};
use crate::agent::codex;
use crate::error::Error;
use crate::log_file::Cause;

/// Runs [`prepare`] for the session, holding its updates until the attach.
pub(super) fn before_attach(
    actor: &mut Actor,
    thread: ThreadId,
    (start, loaded): (SessionStart, bool),
    (mode, guard, picks): (Option<Switch>, Option<PathBuf>, Vec<Pick>),
) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    agent.preparing = true;
    let (id, client, fence) = (agent.id, agent.link.client.clone(), agent.link.fence());
    call(actor, thread, id, async move {
        let (result, picked, fell_back) = prepare(&client, start, (mode, guard), picks).await;
        fence.await;
        AgentReply::Prepared {
            loaded,
            result,
            picked,
            fell_back,
        }
    });
}

/// A session's work before its first prompt, in order: `session/set_mode` to `mode` when given
/// (Codex's ask mode: the host owns the mode, `INITIAL_AGENT_MODE` set it at launch, and this
/// covers an adapter that did not take it; Claude's [`agent::loads::CLAUDE_MODE`], see
/// [`claude_start`], with `acceptEdits` as its fallback when the switch to `auto` fails); then,
/// for Codex, when a hooks object went into `CODEX_CONFIG` (`guard` is the session's folder),
/// `_codex/hooks/list` once, which must show the guard trusted; then
/// the model and effort `picks` (`picks::apply`), whose failures fail nothing. The start comes
/// back with the mode and options it now has, and whether the mode switch fell back.
async fn prepare(
    client: &AcpClient,
    mut start: SessionStart,
    (mode, guard): (Option<Switch>, Option<PathBuf>),
    picks: Vec<Pick>,
) -> (Result<SessionStart, Error>, Picked, bool) {
    let fell_back = match checks(client, &mut start, (mode, guard)).await {
        Ok(fell_back) => fell_back,
        Err(error) => return (Err(error), Picked::default(), false),
    };
    let picked = super::super::picks::apply(client, &mut start, picks).await;
    (Ok(start), picked, fell_back)
}

/// A `session/set_mode` before the attach: to `mode`, and, should the agent refuse it, to
/// `fallback` when there is one (Claude's `acceptEdits` for `auto`), else the launch fails.
#[derive(Debug)]
pub(super) struct Switch {
    pub(super) mode: AgentChoiceId,
    pub(super) fallback: Option<AgentChoiceId>,
}

/// [`prepare`]'s mode switch and guard check; `true` when the switch fell back.
async fn checks(
    client: &AcpClient,
    start: &mut SessionStart,
    (mode, guard): (Option<Switch>, Option<PathBuf>),
) -> Result<bool, Error> {
    let mut fell_back = false;
    if let Some(Switch { mode, fallback }) = mode {
        let mode = match (
            client.set_mode(&start.session, mode.clone()).await,
            fallback,
        ) {
            (Ok(()), _) => Some(mode),
            (Err(error), None) => return Err(error.into()),
            (Err(error), Some(fallback)) => {
                tracing::warn!(error = %Cause(&error), to = %fallback.as_str(), "the agent refused its mode; falling back");
                fell_back = true;
                let current = start.modes.as_ref().map(|modes| modes.current.as_str());
                if current == Some(fallback.as_str()) {
                    None
                } else {
                    client.set_mode(&start.session, fallback.clone()).await?;
                    Some(fallback)
                }
            }
        };
        if let (Some(mode), Some(modes)) = (mode, &mut start.modes) {
            modes.current = CompactString::from(mode.as_str());
        }
    }
    if let Some(cwd) = guard {
        let answer = client.codex_hooks_list(&cwd).await?;
        if !codex::guard_trusted(&answer) {
            return Err(codex::guard_untrusted(&answer));
        }
    }
    Ok(fell_back)
}

/// [`prepare`] ended well: a Claude switch to `auto` is judged (`claude_mode::prepared`,
/// before the picks may drop held updates), the picks' outcome (`picks::after`), then the
/// attach; or first another round of [`prepare`] with only the picks made meanwhile
/// (`picks::late`), so the first prompt, which waits for the attach, runs with them.
pub(crate) fn on_prepared(
    actor: &mut Actor,
    thread: ThreadId,
    start: SessionStart,
    (loaded, fell_back): (bool, bool),
    picked: &Picked,
) {
    claude_mode::prepared(actor, thread, fell_back);
    super::super::picks::after(actor, thread, picked, &start.config_options);
    let late = super::super::picks::late(actor, thread, &start.config_options, picked);
    if !late.is_empty() {
        before_attach(actor, thread, (start, loaded), (None, None, late));
        return;
    }
    attach(actor, thread, start, loaded);
}
