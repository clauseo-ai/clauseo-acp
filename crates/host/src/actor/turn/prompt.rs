//! Step 5: the prompt, with the undo note (the files put back since the last turn) first.

use clauseo_protocol::{ContentBlock, ErrorCode, StreamId, ThreadId, TurnId, ViewEntry};
use futures::future::{AbortHandle, Abortable};

use super::super::Actor;
use super::super::input::Input;
use super::agents::Stage;
use super::{AgentReply, End, Phase, TurnInput, current, end, spawn, state};
use crate::undo_note::{self, NoteInput};

/// Step 5: the prompt, with the undo note first.
pub(super) fn prompt(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let note = take_note(actor, thread);
    let agent = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .filter(|agent| agent.stage == Stage::Attached)
        .and_then(|agent| {
            let session = agent.session.clone()?;
            Some((
                agent.id,
                agent.link.client.clone(),
                agent.link.fence(),
                session,
            ))
        });
    let Some((id, client, fence, session)) = agent else {
        if let Some(current) = current(actor, thread) {
            current.phase = Phase::Prompting;
        }
        let end = End::failed(
            ErrorCode::AgentExited,
            Some("the agent is gone".to_owned()),
            None,
        );
        end::end_turn(actor, thread, end);
        return;
    };
    let Some(current) = current(actor, thread) else {
        return;
    };
    current.phase = Phase::Prompting;
    let blocks = std::mem::take(&mut current.prompt);
    let mut content = Vec::with_capacity(blocks.len() + 1);
    if let Some(text) = note {
        content.push(ContentBlock::Text { text });
    }
    content.extend(blocks);
    let (abort, registration) = AbortHandle::new_pair();
    current.prompt_call = Some(abort);
    let call = Abortable::new(
        async move {
            let result = client.prompt(&session, turn, content).await;
            fence.await;
            result
        },
        registration,
    );
    let post = actor.post.clone();
    spawn(actor, async move {
        // Aborted: the cancel fallback dropped the prompt; nothing to post.
        if let Ok(result) = call.await {
            let reply = AgentReply::Prompt { turn, result };
            let input = TurnInput::Agent {
                thread,
                agent: id,
                reply,
            };
            let _gone = post.try_send(Input::Turn(input));
        }
    });
}

/// The undo note's text for the next prompt, and the note cleared for the turn starting now.
fn take_note(actor: &mut Actor, thread: ThreadId) -> Option<String> {
    ensure_note(actor, thread);
    let paths = state(actor, thread)?.note.replace(Vec::new())?;
    undo_note::note_text(&paths)
}

/// Reads the note from the loaded state the first time it is needed: the files undone since
/// the last turn's step.
pub(super) fn ensure_note(actor: &mut Actor, thread: ThreadId) {
    if state(actor, thread).is_none_or(|state| state.note.is_some()) {
        return;
    }
    let paths = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .map(|state| {
            let inputs = state.entries().values().filter_map(|entry| {
                if matches!(entry, ViewEntry::Step(_)) {
                    Some(NoteInput::TurnStarted)
                } else if let ViewEntry::Marker(marker) = entry
                    && let clauseo_protocol::MarkerKind::StepUndone { restored, .. } = &marker.kind
                {
                    Some(NoteInput::Undone { restored })
                } else {
                    None
                }
            });
            undo_note::paths_since_last_turn(inputs)
        })
        .unwrap_or_default();
    if let Some(state) = state(actor, thread) {
        state.note = Some(paths);
    }
}
