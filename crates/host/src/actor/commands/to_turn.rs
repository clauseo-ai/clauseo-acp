//! The commands that go to their thread's turn: `run_command` hands each here, and it reaches
//! `turn::command` as the matching `TurnCommand` when its thread is listed.

use clauseo_protocol::{HostCommand, ThreadId};

use super::super::turn::{self, TurnCommand};
use super::{CommandCtx, listed_thread, not_built};
use crate::actor::Actor;

/// The commands that go to their thread's turn, each as the matching `TurnCommand`.
/// `run_command` sends only these here. The chain is `if let`, not a `match`, so it needs no
/// wildcard arm for the commands `run_command` keeps; anything else is not built.
pub(super) fn run_turn_command(actor: &mut Actor, ctx: &CommandCtx, command: HostCommand) {
    let (thread, turn) = if let HostCommand::SendPrompt {
        thread,
        content,
        delivery,
    } = command
    {
        (thread, TurnCommand::SendPrompt { content, delivery })
    } else if let HostCommand::Cancel { thread } = command {
        (thread, TurnCommand::Cancel)
    } else if let HostCommand::AnswerPermission {
        thread,
        request,
        option_id,
    } = command
    {
        (thread, TurnCommand::AnswerPermission { request, option_id })
    } else if let HostCommand::AnswerElicitation {
        thread,
        elicitation,
        response,
    } = command
    {
        let turn = TurnCommand::AnswerElicitation {
            elicitation,
            response,
        };
        (thread, turn)
    } else if let HostCommand::SetConfigOption {
        thread,
        option_id,
        value,
    } = command
    {
        (thread, TurnCommand::SetConfigOption { option_id, value })
    } else if let HostCommand::SetMode { thread, mode_id } = command {
        (thread, TurnCommand::SetMode { mode_id })
    } else if let HostCommand::UndoStep {
        thread,
        turn,
        files,
    } = command
    {
        (thread, TurnCommand::UndoStep { turn, files })
    } else if let HostCommand::EditQueuedPrompt { thread, turn, text } = command {
        (thread, TurnCommand::EditQueuedPrompt { turn, text })
    } else if let HostCommand::RemoveQueuedPrompt { thread, turn } = command {
        (thread, TurnCommand::RemoveQueuedPrompt { turn })
    } else if let HostCommand::SendQueuedNow { thread, turn } = command {
        (thread, TurnCommand::SendQueuedNow { turn })
    } else if let HostCommand::ResumeQueue { thread } = command {
        (thread, TurnCommand::ResumeQueue)
    } else {
        return not_built(actor, ctx);
    };
    turn_command(actor, ctx, thread, turn);
}

/// A turn command for a known thread goes to the turn path; an unknown one is refused.
fn turn_command(actor: &mut Actor, ctx: &CommandCtx, thread: ThreadId, command: TurnCommand) {
    match listed_thread(actor, thread) {
        Ok(_) => turn::command(actor, ctx, thread, command),
        Err(error) => actor.finish_command(ctx.id, Err(error)),
    }
}
