//! The cause a turn that never started ends with (`TurnEnded.cause`), by the path that ends it:
//! a Stop on a turn waiting to start, a removal, a send now over a waiting turn, a sign-in's
//! stop and an install that cancels a waiting message, whose copy is queued again, held. A
//! started turn ends with no cause.

use clauseo_protocol::{
    CommandOutcome, ContentBlock, DeliveryRequest, DomainEvent, HostCommand, PromptBlock,
    QueueHoldReason, StopReason, ThreadId, TurnEnded, TurnId, UnsentCause,
};

use super::super::{End, TurnPhase, cancel, end, queue};
use super::failures::until_ended;
use super::rig::Rig;
use crate::actor::input::Input;
use crate::actor::turn::cancel_all;
use crate::matter_worker::MatterReply;

/// The agent's steps after its handshake: one prompt, answered `end_turn`.
const PROMPT_ENDS: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

/// Sends `text` to `thread` as command `n`, queued or sent now; returns its turn.
fn send(rig: &mut Rig, n: u128, thread: ThreadId, delivery: DeliveryRequest) -> TurnId {
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Then clause 5?".to_owned(),
        }],
        delivery,
        config: None,
    };
    match rig.dispatch(n, command) {
        Ok(CommandOutcome::PromptAccepted { turn, .. }) => turn,
        other => panic!("SendPrompt answered {other:?}"),
    }
}

/// The `TurnEnded` of `turn` in `thread`'s log.
fn ended(rig: &mut Rig, thread: ThreadId, turn: TurnId) -> TurnEnded {
    let found = rig
        .log(thread)
        .into_iter()
        .find(|event| matches!(event, DomainEvent::TurnEnded(t) if t.turn == turn));
    match found {
        Some(DomainEvent::TurnEnded(ended)) => ended,
        other => panic!("no TurnEnded for {turn}: {other:?}"),
    }
}

/// Whether `thread`'s log has a `TurnStarted` of `turn`.
fn started(rig: &mut Rig, thread: ThreadId, turn: TurnId) -> bool {
    rig.log(thread)
        .iter()
        .any(|event| matches!(event, DomainEvent::TurnStarted(s) if s.turn == turn))
}

#[test]
fn a_stop_on_a_turn_waiting_to_start_ends_stopped() {
    let mut rig = Rig::start("unsent-stop", "");
    let thread = rig.thread();
    let waiting = rig.current_turn(thread, TurnPhase::Queued);
    let cancelled = rig.dispatch(40, HostCommand::Cancel { thread });
    assert_eq!(cancelled.unwrap(), CommandOutcome::Done);
    rig.settle_commits();

    let event = ended(&mut rig, thread, waiting);
    assert_eq!(event.stop, StopReason::Cancelled);
    assert_eq!(event.cause, Some(UnsentCause::Stopped));
    assert!(!started(&mut rig, thread, waiting));
    rig.finish();
}

#[test]
fn a_stop_during_the_checkpoint_before_the_turn_ends_stopped() {
    let mut rig = Rig::start("unsent-checkpoint-stop", PROMPT_ENDS);
    let thread = rig.thread();
    // The before-turn checkpoint's answer is held: the turn is checkpointing when the Stop comes.
    let held = |_: &crate::actor::Actor, input: &Input| {
        matches!(
            input,
            Input::Matter {
                reply: MatterReply::BeforeTurn(_),
                ..
            }
        )
    };
    let sent = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Then clause 5?".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
        config: None,
    };
    let turn = match rig.dispatch_holding(30, sent, held) {
        Ok(CommandOutcome::PromptAccepted { turn, .. }) => turn,
        other => panic!("SendPrompt answered {other:?}"),
    };
    // The launch ends, and the checkpoint is asked for: its answer is held there.
    rig.run_holding("the turn checkpointing", held, |actor| {
        let current = actor.threads[&thread].turn.current.as_ref();
        current.is_some_and(|current| current.phase == TurnPhase::Checkpointing)
    });

    assert_eq!(
        rig.dispatch(31, HostCommand::Cancel { thread }).unwrap(),
        CommandOutcome::Done
    );
    rig.release_held();
    until_ended(&mut rig, thread);
    rig.settle_commits();

    let event = ended(&mut rig, thread, turn);
    assert_eq!(event.stop, StopReason::Cancelled);
    assert_eq!(event.cause, Some(UnsentCause::Stopped));
    assert!(!started(&mut rig, thread, turn));
    rig.finish();
}

#[test]
fn removing_a_queued_prompt_ends_removed() {
    let mut rig = Rig::start("unsent-remove", "");
    let thread = rig.thread();
    rig.running_turn(thread);
    let queued = send(&mut rig, 30, thread, DeliveryRequest::Queue);
    rig.settle_commits();

    let removed = HostCommand::RemoveQueuedPrompt {
        thread,
        turn: queued,
    };
    assert_eq!(rig.dispatch(31, removed).unwrap(), CommandOutcome::Done);
    rig.settle_commits();

    let event = ended(&mut rig, thread, queued);
    assert_eq!(event.stop, StopReason::Cancelled);
    assert_eq!(event.cause, Some(UnsentCause::Removed));
    rig.finish();
}

#[test]
fn a_send_now_over_a_waiting_turn_ends_sent_now() {
    let mut rig = Rig::start("unsent-send-now", "");
    let thread = rig.thread();
    let waiting = rig.current_turn(thread, TurnPhase::Queued);
    let next = send(&mut rig, 30, thread, DeliveryRequest::Queue);
    rig.settle_commits();

    let sent_now = HostCommand::SendQueuedNow { thread, turn: next };
    assert_eq!(rig.dispatch(31, sent_now).unwrap(), CommandOutcome::Done);
    rig.settle_commits();

    let event = ended(&mut rig, thread, waiting);
    assert_eq!(event.stop, StopReason::Cancelled);
    assert_eq!(event.cause, Some(UnsentCause::SentNow));
    rig.finish();
}

#[test]
fn a_sign_in_stop_on_a_waiting_turn_ends_sign_in_and_its_copy_waits_for_the_sign_in() {
    let mut rig = Rig::start("unsent-sign-in", "");
    let thread = rig.thread();
    let waiting = rig.current_turn(thread, TurnPhase::Queued);
    // What `stop_agents` does to the turn it cancels (`sign_in.rs`).
    rig.actor
        .threads
        .get_mut(&thread)
        .unwrap()
        .turn
        .stopped_for_sign_in = Some(waiting);
    cancel::cancel_current(&mut rig.actor, thread, cancel::why::SIGN_IN);
    rig.settle_commits();

    let event = ended(&mut rig, thread, waiting);
    assert_eq!(event.cause, Some(UnsentCause::SignIn));
    let copy = queue::queued_turns(&rig.actor, thread);
    assert_eq!(copy.len(), 1, "{copy:?}");
    assert_eq!(
        queue::queue_hold(&rig.actor, thread),
        Some(QueueHoldReason::SignIn)
    );
    rig.finish();
}

#[test]
fn an_install_cancelling_a_waiting_message_leaves_a_held_copy() {
    let mut rig = Rig::start("unsent-install", "");
    let thread = rig.thread();
    let waiting = rig.current_turn(thread, TurnPhase::Queued);
    let text = vec![ContentBlock::Text {
        text: "Draft the notice.".to_owned(),
    }];
    rig.actor
        .threads
        .get_mut(&thread)
        .unwrap()
        .turn
        .current
        .as_mut()
        .unwrap()
        .prompt = text.clone();
    cancel_all(&mut rig.actor);
    rig.settle_commits();

    let event = ended(&mut rig, thread, waiting);
    assert_eq!(event.stop, StopReason::Cancelled);
    assert_eq!(event.cause, Some(UnsentCause::Update));
    assert!(!started(&mut rig, thread, waiting));
    // The copy: a new `UserMessage` with the same content, queued, held for the lawyer (not for
    // a sign-in, which nothing waits for here) and not started.
    let copies: Vec<TurnId> = queue::queued_turns(&rig.actor, thread);
    assert_eq!(copies.len(), 1, "{copies:?}");
    let copy = copies[0];
    assert_ne!(copy, waiting);
    let copied = rig.log(thread).into_iter().any(
        |event| matches!(event, DomainEvent::UserMessage(m) if m.turn == copy && m.content == text),
    );
    assert!(copied, "no copy of the message was logged");
    assert_eq!(
        queue::queue_hold(&rig.actor, thread),
        Some(QueueHoldReason::Paused)
    );
    assert!(!started(&mut rig, thread, copy));
    rig.finish();
}

#[test]
fn a_started_turn_ends_with_no_cause() {
    let mut rig = Rig::start("unsent-started", "");
    let thread = rig.thread();
    let running = rig.running_turn(thread);
    end::end_turn(&mut rig.actor, thread, End::cancelled(None));
    until_ended(&mut rig, thread);
    rig.settle_commits();

    let event = ended(&mut rig, thread, running);
    assert_eq!(event.stop, StopReason::Cancelled);
    assert_eq!(event.cause, None);
    rig.finish();
}
