//! Step 5: the prompt, with the notes first: the offline-shell note a steer could not deliver
//! (`shell_network.rs`), then the undo note (the files put back since the last turn).
//!
//! The note is added here, after `SendPrompt` checked the question alone, so a Codex prompt is
//! checked against Codex's cap again as it will be sent (`codex_cap::refuse`): over
//! it, the turn ends failed with `prompt.too_long` before anything reaches the agent, the queue
//! held, and the note kept for the next prompt.
//!
//! The undo note is held aside while its prompt is out, and put back unless the agent took that
//! prompt (`note.rs`); the offline-shell note is spent once sent, or once the agent is gone.

use clauseo_protocol::{ContentBlock, ErrorCode, ThreadId, TurnId};
use futures::future::{AbortHandle, Abortable, join};

use super::super::Actor;
use super::super::input::Input;
use super::agents::Stage;
use super::key_retry;
use super::{AgentReply, BeforePrompt, End, TurnInput, TurnPhase, current, end, spawn, state};
use crate::agent::{codex, leftovers};
use crate::log_file::LIFECYCLE;

/// Step 5: the prompt, with the undo note first.
pub(super) fn prompt(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let note = super::note::text(actor, thread);
    let agent = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .filter(|agent| agent.stage == Stage::Attached)
        .and_then(|agent| {
            let session = agent.session.clone()?;
            Some((
                (
                    agent.id,
                    agent.codex.is_some().then(|| agent.folder.clone()),
                ),
                (agent.link.client.clone(), agent.link.descendants().clone()),
                agent.link.fence(),
                session,
            ))
        });
    let Some(((id, check), (client, descendants), fence, session)) = agent else {
        // The undo note stays: no prompt reached the agent.
        if let Some(state) = state(actor, thread) {
            state.offline_owed = false;
        }
        if let Some(current) = current(actor, thread) {
            current.phase = TurnPhase::Prompting;
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
    current.phase = TurnPhase::Prompting;
    let mut content = Vec::with_capacity(current.prompt.len() + 1);
    if let Some(text) = note {
        content.push(ContentBlock::Text { text });
    }
    content.extend(current.prompt.iter().cloned());
    let cancelled = current.cancelled;
    // The text exactly as it goes out, the note in: over Codex's cap, nothing is sent.
    if let Err(error) = super::codex_cap::refuse(actor, thread, &content) {
        tracing::warn!(
            %thread,
            %turn,
            error = %crate::log_file::Cause(&error),
            "a Codex prompt with its undo note was refused before it was sent"
        );
        let end = if cancelled {
            End::cancelled(None)
        } else {
            End::failed(error.code(), Some(error.to_string()), None)
        };
        end::end_turn(actor, thread, end);
        return;
    }
    super::note::sent(actor, thread, turn);
    let retry = key_retry::for_call(actor, thread);
    let Some(current) = super::current(actor, thread) else {
        return;
    };
    let stopped = std::rc::Rc::clone(&current.stopped);
    super::state::prompt_sent(actor, thread);
    // The prompt goes out: the case-law relay's turn caps start again (R24), and while the
    // window says the computer is offline, the turn is watched (`offline.rs`).
    super::launch_checks::relay::turn_started(actor, thread);
    super::offline::turn_started(actor, thread);
    let Some(current) = super::current(actor, thread) else {
        return;
    };
    let retry = retry.unless_stopped(stopped);
    let (abort, registration) = AbortHandle::new_pair();
    current.prompt_call = Some(abort);
    let before = BeforePrompt::default();
    current.before_prompt = Some(std::rc::Rc::clone(&before));
    let clock = std::sync::Arc::clone(&actor.clock);
    let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
    let no_git = no_git_of(actor, thread);
    let list_clock = std::sync::Arc::clone(&actor.clock);
    let call = Abortable::new(
        async move {
            // A Codex agent's matter folder is checked for `.codex` again first, and every
            // agent's `git` that does nothing, and the agent's descendants are listed, all off
            // the actor, before the prompt goes out: a cancelled turn ends what is new since this
            // listing.
            let (checked, listed) = join(
                codex::prepare::recheck(clock, matter, check, no_git),
                leftovers::snapshot(descendants, list_clock),
            )
            .await;
            *before.borrow_mut() = listed;
            if let Err(error) = checked {
                return AgentReply::PromptRefused { turn, error };
            }
            tracing::info!(target: LIFECYCLE, %thread, %turn, "prompt sent");
            let result = retry
                .run(&client, || client.prompt(&session, turn, content.clone()))
                .await;
            fence.await;
            AgentReply::Prompt { turn, result }
        },
        registration,
    );
    let post = actor.post.clone();
    spawn(actor, async move {
        // Aborted: the cancel fallback dropped the prompt; nothing to post.
        if let Ok(reply) = call.await {
            let input = TurnInput::Agent {
                thread,
                agent: id,
                reply,
            };
            let _gone = post.try_send(Input::Turn(input));
        }
    });
}

/// What a call of `thread`'s agent checks of the `git` that does nothing first
/// (`launch_checks::no_git_check`); nothing for a thread that is gone.
pub(super) fn no_git_of(
    actor: &Actor,
    thread: ThreadId,
) -> Result<Option<crate::agent::no_git::Check>, crate::agent::no_git::Unusable> {
    actor.threads.get(&thread).map_or(Ok(None), |entry| {
        super::launch_checks::no_git_check(actor, &entry.row.agent)
    })
}
