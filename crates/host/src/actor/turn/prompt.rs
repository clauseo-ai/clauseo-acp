//! Step 5: the prompt, with the undo note first (the files put back since the last turn).
//!
//! The note is added here, after `SendPrompt` checked the question alone, so a Codex prompt is
//! checked against Codex's cap again as it will be sent (`codex_cap::refuse`): over
//! it, the turn ends failed with `prompt.too_long` before anything reaches the agent, the queue
//! held, and the note kept for the next prompt.
//!
//! The undo note is held aside while its prompt is out, and put back unless the agent took that
//! prompt (`note.rs`).

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
    let options = super::picks::thread_options(actor, thread);
    let picks = super::prompt_picks::for_prompt(actor, thread, turn, &options);
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
    let resend = super::codex_reviewer::Resend::for_prompt(check.is_some(), &stopped);
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
    let list_clock = std::sync::Arc::clone(&actor.clock);
    let picked_post = actor.post.clone();
    let call = Abortable::new(
        async move {
            // A Codex agent's matter folder is checked for `.codex` again first, and the agent's
            // descendants are listed, both off the actor, before the prompt goes out: a
            // cancelled turn ends what is new since this listing.
            let (checked, listed) = join(
                codex::prepare::recheck(clock, matter, check),
                leftovers::listing(descendants, list_clock, leftovers::SNAPSHOT_WAIT),
            )
            .await;
            *before.borrow_mut() = match listed {
                leftovers::Listed::Failed => None,
                kept @ (leftovers::Listed::Taken(_) | leftovers::Listed::Late(_)) => Some(kept),
            };
            if let Err(error) = checked {
                return AgentReply::PromptRefused { turn, error };
            }
            if let Some(options) = set_picks(&client, &session, picks).await {
                let reply = AgentReply::PromptPicked(options);
                let input = TurnInput::Agent {
                    thread,
                    agent: id,
                    reply,
                };
                let _gone = picked_post.try_send(Input::Turn(input));
            }
            tracing::info!(target: LIFECYCLE, %thread, %turn, "prompt sent");
            let send = || client.prompt(&session, turn, content.clone());
            let result = retry.run(&client, send).await;
            let (result, fell_back) = resend.run(&client, &session, result, send).await;
            fence.await;
            AgentReply::Prompt {
                turn,
                result,
                fell_back,
            }
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

/// Sets `picks` on `session`, in order; the options of the agent's last answer, `None` when
/// none was taken. A pick the agent refuses is skipped; the prompt goes out after them all the
/// same.
async fn set_picks(
    client: &clauseo_acp::AcpClient,
    session: &clauseo_protocol::AgentSessionId,
    picks: Vec<super::picks::Pick>,
) -> Option<Vec<clauseo_protocol::ConfigOption>> {
    let mut last = None;
    for (option, value) in picks {
        let select = clauseo_protocol::ConfigValue::Select(value.clone());
        match client
            .set_config_option(session, option.clone(), select)
            .await
        {
            Ok(options) => {
                tracing::info!(target: LIFECYCLE, %option, %value, "a prompt's pick was set");
                last = Some(options);
            }
            Err(error) => {
                tracing::info!(%option, error = %crate::log_file::Cause(&error), "a prompt's pick was not set");
            }
        }
    }
    last
}
