//! A session's work between its start's answer and its attach (`session.rs`): the mode
//! switch, Codex's guard check and the model and effort picks, held updates meanwhile; then, for
//! picks made while it ran, another round (`picks::late`), else the attach.

use std::path::PathBuf;

use clauseo_acp::{AcpClient, SessionStart};
use clauseo_protocol::ThreadId;

use super::super::super::Actor;
use super::super::agents::call;
use super::super::picks::{Pick, Picked};
use super::super::{AgentReply, state};
use super::claude_mode::{Switch, Switched};
use super::{attach, claude_mode};
use crate::agent::codex;
use crate::error::Error;

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
        let (result, picked, switched) = prepare(&client, start, (mode, guard), picks).await;
        fence.await;
        AgentReply::Prepared {
            loaded,
            result,
            picked,
            switched,
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
/// back with the mode and options it now has, and what a Claude mode switch did, for its
/// read-back (`claude_mode::prepared`). No prompt goes out before the attach that follows.
async fn prepare(
    client: &AcpClient,
    mut start: SessionStart,
    (mode, guard): (Option<Switch>, Option<PathBuf>),
    picks: Vec<Pick>,
) -> (Result<SessionStart, Error>, Picked, Option<Switched>) {
    let switched = match checks(client, &mut start, (mode, guard)).await {
        Ok(switched) => switched,
        Err(error) => return (Err(error), Picked::default(), None),
    };
    let picked = super::super::picks::apply(client, &mut start, picks).await;
    (Ok(start), picked, switched)
}

/// [`prepare`]'s mode switch (`claude_mode::switch`) and guard check.
async fn checks(
    client: &AcpClient,
    start: &mut SessionStart,
    (mode, guard): (Option<Switch>, Option<PathBuf>),
) -> Result<Option<Switched>, Error> {
    let switched = match mode {
        Some(mode) => claude_mode::switch(client, start, mode).await?,
        None => None,
    };
    if let Some(cwd) = guard {
        let answer = client.codex_hooks_list(&cwd).await?;
        if !codex::guard_trusted(&answer) {
            return Err(codex::guard_untrusted(&answer));
        }
    }
    Ok(switched)
}

/// [`prepare`] ended well: a Claude mode switch is read back (`claude_mode::prepared`, before
/// the picks may drop held updates; a refused one ends the launch here), the picks' outcome
/// (`picks::after`), then the attach; or first another round of [`prepare`] with only the picks made meanwhile
/// (`picks::late`), so the first prompt, which waits for the attach, runs with them.
pub(crate) fn on_prepared(
    actor: &mut Actor,
    thread: ThreadId,
    mut start: SessionStart,
    (loaded, switched): (bool, Option<Switched>),
    picked: &Picked,
) {
    if !claude_mode::prepared(actor, thread, switched, &mut start) {
        return;
    }
    super::super::picks::after(actor, thread, picked, &start.config_options);
    let late = super::super::picks::late(actor, thread, &start.config_options, picked);
    if !late.is_empty() {
        before_attach(actor, thread, (start, loaded), (None, None, late));
        return;
    }
    attach(actor, thread, start, loaded);
}
