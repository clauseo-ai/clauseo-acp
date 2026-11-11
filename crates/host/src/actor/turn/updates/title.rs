//! An agent's session title, split from `updates.rs` for its length.

use clauseo_protocol::{CompactString, RenameSource, ThreadId, ThreadRenamed};

use super::super::super::{Actor, title_source};
use super::super::out::Out;
use super::super::{Then, current, lost};
use super::{Context, agent};
use crate::error::Error;

/// A session title: `ThreadRenamed { by: Agent }` and the row, when it is a valid title the
/// thread takes (see `title_source`). The agent's titles count only until a turn of the thread
/// has ended (`state::turn_ended`): after the first answer the name settles, and the title is
/// ignored (D-070). A title the thread already has from the agent makes no event, so the replay
/// Codex sends on load adds nothing.
pub(super) fn changed(
    actor: &mut Actor,
    thread: ThreadId,
    title: Option<String>,
    context: Context,
) -> Result<(), Error> {
    let Some(title) = title else {
        tracing::debug!(%thread, "the agent cleared its session title");
        return Ok(());
    };
    // An update outside the live turn comes after the prompt's answer when a prompt is in
    // flight: the answer's reply may not be handled yet.
    let idle = matches!(context, Context::Outside { replay: false });
    if super::super::state::turn_ended(actor, thread, idle) {
        tracing::debug!(%thread, "a turn has ended; the agent's session title is ignored");
        return Ok(());
    }
    let prompt = current(actor, thread).map(|current| current.prompt.as_slice());
    let Some(title) = title_source::agent_title(thread, &title, prompt) else {
        return Ok(());
    };
    let entry = actor.threads.get(&thread).ok_or_else(|| lost("a thread"))?;
    let (source, has_title) = (entry.row.title_source, entry.row.title.is_some());
    if !title_source::title_may_change(source, has_title, RenameSource::Agent) {
        // No event came between the chunks, so the message run goes on.
        tracing::debug!(%thread, "the agent's title does not replace the thread's title");
        return Ok(());
    }
    if source == Some(RenameSource::Agent) && entry.row.title.as_deref() == Some(title.as_str()) {
        return Ok(());
    }
    let mut out = Out::new(actor, thread, None)?;
    out.push(
        actor,
        ThreadRenamed {
            title: CompactString::from(title.as_str()),
            by: RenameSource::Agent,
        },
    )?;
    let now = actor.now().wall;
    let entry = actor
        .threads
        .get_mut(&thread)
        .ok_or_else(|| lost("a thread"))?;
    entry.row.title = Some(title.as_str().to_owned());
    entry.row.title_source = Some(RenameSource::Agent);
    entry.row.updated = now;
    out.draft.thread_row(entry.row.clone());
    out.send(actor, Then::Nothing)?;
    if let Some(agent) = agent(actor, thread) {
        agent.run = None;
    }
    Ok(())
}
