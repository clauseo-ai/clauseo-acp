//! The commit that puts a new turn in a thread's queue (a prompt queued, sent now, or a steer
//! that waits as the next turn), and the tag a queue command's commit carries.

use clauseo_protocol::{
    CommandOutcome, ContentBlock, DeliveryRequest, MessageId, StreamId, ThreadId, TurnId,
    TurnQueued,
};

use super::super::super::commands::{AfterCommand, CommandCtx};
use super::super::super::{Actor, Tag};
use super::super::out::Out;
use super::super::{Then, TurnTag, lost};
use super::{After, Queued, ThreadQueue, drop_turn, queue, steer, sync_held};
use crate::build;
use crate::error::Error;

/// How a new turn was asked for, and so how its commit marks where it stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Joins {
    /// As its delivery says: a steer is a send now (`SteerResolved { SentNow }`).
    As(DeliveryRequest),
    /// A steer that waits as the next turn: `SteerResolved { Requeued }`, and its `TurnQueued`
    /// marked `at_head` so a restart keeps it at the head too (`restore.rs`).
    Next,
}

impl Joins {
    fn delivery(self) -> DeliveryRequest {
        match self {
            Self::As(delivery) => delivery,
            Self::Next => DeliveryRequest::Steer,
        }
    }
}

/// One commit of a new turn's `UserMessage`, its `TurnQueued` behind `ahead` of its own
/// thread (behind the thread itself, its thread alone, with none), a steer's `SteerResolved`
/// (`joins`), and the receipt (`PromptAccepted`). `place` puts the turn in the queue before
/// the commit is sent (taken out again if it is refused).
pub(super) fn log_queued(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    ahead: Option<TurnId>,
    blocks: Vec<ContentBlock>,
    joins: Joins,
    place: impl FnOnce(TurnId, MessageId, Vec<ContentBlock>, &mut ThreadQueue) -> After,
) -> Result<(), Error> {
    let now = actor.now().wall;
    let turn: TurnId = actor.ids.mint(now)?;
    let message: MessageId = actor.ids.mint(now)?;
    let delivery = joins.delivery();
    let built = build::user_message(turn, message, None, blocks.clone(), delivery.into());
    let mut waits = TurnQueued::new(thread, turn, thread, ahead)
        .map_err(|error| Error::Internal(error.to_string()))?;
    if joins == Joins::Next {
        waits = waits.at_head();
    }
    let outcome = CommandOutcome::PromptAccepted { turn, message };
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    out.push(actor, built.value)?;
    out.draft.blobs(built.blobs);
    out.push(actor, waits)?;
    match joins {
        Joins::Next => {
            out.push(actor, steer::requeued(message, turn))?;
        }
        Joins::As(DeliveryRequest::Steer) => {
            out.push(actor, steer::sent_now(message, turn))?;
        }
        Joins::As(DeliveryRequest::Queue | DeliveryRequest::SendNow) => {}
    }
    out.draft
        .receipt(ctx.receipt(Some(StreamId::Thread(thread)), outcome, now));
    let after = place(
        turn,
        message,
        blocks,
        queue(actor, thread).ok_or_else(|| lost("a thread"))?,
    );
    let tag = command_tag(ctx, outcome, thread, after);
    let sent = out.send_tagged(actor, tag);
    if sent.is_err() {
        drop_turn(actor, thread, turn);
    }
    sent
}

pub(in crate::actor::turn) fn command_tag(
    ctx: CommandCtx,
    outcome: CommandOutcome,
    thread: ThreadId,
    after: After,
) -> Tag {
    Tag::Command {
        id: ctx.id,
        outcome,
        after: AfterCommand::Turn(TurnTag {
            thread,
            then: Then::Queue(after),
        }),
    }
}

/// A prompt while no turn runs but a cancelled agent-started turn's late window holds starts
/// (`own_turn/late.rs`): it waits in the queue, a send now (or a steer) as the forced head, and
/// starts once the window lets it. It un-holds a queue the stop held as a send with nothing
/// running does (`route`): a send now at once, so the queue moves on after it; a queued prompt
/// into an empty queue at once, and into a held one once its commit is durable, as any append
/// (`committed`).
pub(super) fn wait_late(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    ahead: Option<TurnId>,
    blocks: Vec<ContentBlock>,
    delivery: DeliveryRequest,
) -> Result<(), Error> {
    let forced = delivery != DeliveryRequest::Queue;
    if let Some(queue) = queue(actor, thread)
        && (forced || queue.is_empty())
    {
        queue.held = false;
        sync_held(actor, thread);
    }
    log_queued(
        actor,
        ctx,
        thread,
        ahead,
        blocks,
        Joins::As(delivery),
        |turn, message, content, queue| {
            let queued = Queued {
                turn,
                message,
                content,
                forced,
                host_held: false,
            };
            if forced {
                queue.turns.push_front(queued);
                After::Jumped(turn)
            } else {
                queue.turns.push_back(queued);
                After::Appended(turn)
            }
        },
    )
}
