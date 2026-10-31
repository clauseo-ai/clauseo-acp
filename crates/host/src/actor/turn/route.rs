//! Routing a turn command to its step: `run_command` builds a [`TurnCommand`] from each turn
//! variant of `HostCommand`, and [`command`] matches every one of them.

use clauseo_protocol::{
    AgentChoiceId, ConfigValue, DeliveryRequest, ElicitationId, ElicitationResponse, FileDecision,
    PermissionRequestId, PromptBlock, ThreadId, TurnId,
};

use super::super::Actor;
use super::super::commands::CommandCtx;
use super::{agents, answers, cancel, steps, undo};

/// A command of the turn path with its thread pulled out: `run_command` builds one from each
/// turn variant of `HostCommand`, so a variant it routes here without an arm below does not
/// compile.
#[derive(Debug)]
pub(crate) enum TurnCommand {
    SendPrompt {
        content: Vec<PromptBlock>,
        delivery: DeliveryRequest,
    },
    Cancel,
    AnswerPermission {
        request: PermissionRequestId,
        option_id: AgentChoiceId,
    },
    AnswerElicitation {
        elicitation: ElicitationId,
        response: ElicitationResponse,
    },
    SetConfigOption {
        option_id: AgentChoiceId,
        value: ConfigValue,
    },
    SetMode {
        mode_id: AgentChoiceId,
    },
    UndoStep {
        turn: TurnId,
        files: Vec<FileDecision>,
    },
}

/// A turn command, after its shape, principal and receipt checks passed and its thread is
/// known. It ends in `actor.finish_command(ctx.id, ..)`, at once for a refusal (no receipt) or
/// from the continuation of a commit that carries the receipt.
pub(crate) fn command(actor: &mut Actor, ctx: &CommandCtx, thread: ThreadId, command: TurnCommand) {
    let ctx = *ctx;
    let done = match command {
        TurnCommand::SendPrompt { content, delivery } => {
            steps::send_prompt(actor, ctx, thread, content, delivery.into())
        }
        TurnCommand::Cancel => cancel::cancel_command(actor, ctx, thread),
        TurnCommand::AnswerPermission { request, option_id } => {
            answers::answer_permission(actor, ctx, thread, request, option_id)
        }
        TurnCommand::AnswerElicitation {
            elicitation,
            response,
        } => answers::answer_elicitation(actor, ctx, thread, elicitation, response),
        TurnCommand::SetConfigOption { option_id, value } => {
            agents::ensure(
                actor,
                thread,
                agents::Waiting::Config(ctx, option_id, value),
            );
            Ok(())
        }
        TurnCommand::SetMode { mode_id } => {
            agents::ensure(actor, thread, agents::Waiting::Mode(ctx, mode_id));
            Ok(())
        }
        TurnCommand::UndoStep { turn, files } => undo::undo_step(actor, ctx, thread, turn, files),
    };
    if let Err(error) = done {
        actor.finish_command(ctx.id, Err(error));
    }
}
