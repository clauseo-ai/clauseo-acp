//! Store failures on the turn path, with a real store whose commits a test makes fail: the
//! cancel after a failure reads the rolled-back sent view, a cancel the failure refused is
//! logged once the stream drains, the end of a turn is retried once and blocks new prompts
//! while it is not durable, and held text survives a blocked stream.

use clauseo_protocol::{
    CheckpointKind, DomainEvent, ErrorCode, HostCommand, HostError, PromptBlock, StopReason,
    StreamId, TextKind, ThreadId, ToolCallStarted, ToolCallStatus, ToolKind, TurnId,
};

use super::rig::{Rig, kind};
use crate::actor::input::Input;
use crate::actor::turn::{CANCEL_WAIT, End, Phase, end, out};
use crate::coalesce::TextIn;
use crate::pipeline::Draft;
use crate::pipeline::tests::support::notice;

/// A pending tool call of `turn` the agent knows as `acp_id`.
fn started(rig: &mut Rig, turn: TurnId, acp_id: &str) -> ToolCallStarted {
    let now = rig.actor.now().wall;
    ToolCallStarted {
        turn,
        tool_call: rig.actor.ids.mint(now).unwrap(),
        acp_id: acp_id.into(),
        title: "Read brief.txt".into(),
        kind: ToolKind::Read,
        tool_name: None,
        status: ToolCallStatus::Pending,
        locations: Vec::new(),
        raw_input: None,
        raw_output: None,
        content: Vec::new(),
    }
}

/// The tool calls the log updates, in order.
fn updated(log: &[DomainEvent]) -> Vec<clauseo_protocol::ToolCallId> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::ToolCallUpdated(updated) = event {
                Some(updated.tool_call)
            } else {
                None
            }
        })
        .collect()
}

/// A running turn with one durable tool call, and a second whose commit the store refuses.
fn failing_tool_call(rig: &mut Rig) -> (ThreadId, TurnId, clauseo_protocol::ToolCallId) {
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    let first = started(rig, turn, "tc-1");
    let durable = first.tool_call;
    rig.commit(thread, vec![first.into()]);
    rig.settle_commits();
    let second = started(rig, turn, "tc-2");
    let refused = second.tool_call;
    rig.fail(
        1,
        move |event| matches!(event, DomainEvent::ToolCallStarted(s) if s.tool_call == refused),
    );
    rig.commit(thread, vec![second.into()]);
    (thread, turn, durable)
}

/// Handles inputs until `thread` has no turn and nothing is in flight.
fn until_ended(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("the turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && actor.work.commits == 0 && actor.work.matter_jobs == 0
    });
}

/// The turn's last two events: `TurnFailed` for the store failure, then `TurnEnded { Failed }`.
fn assert_failed_end(log: &[DomainEvent]) {
    let [
        ..,
        DomainEvent::TurnFailed(failed),
        DomainEvent::TurnEnded(ended),
    ] = log
    else {
        panic!("{:?}", log.iter().map(kind).collect::<Vec<_>>());
    };
    assert_eq!(ended.stop, StopReason::Failed);
    assert_ne!(failed.code, ErrorCode::AgentExited);
    assert!(failed.detail.is_some());
}

#[test]
fn cancel_after_a_failed_commit_reads_the_rolled_back_view() {
    let mut rig = Rig::start("turn-cancel-rolled-back", "");
    let (thread, turn, durable) = failing_tool_call(&mut rig);
    rig.settle_commits();

    // The cancel closes the durable tool call only; the refused one was never logged.
    assert_eq!(updated(&rig.log(thread)), [durable]);
    let loaded = rig.actor.loaded(StreamId::Thread(thread)).unwrap();
    assert!(loaded.sent.settled(), "the sent view is the log's");
    assert_eq!(loaded.sent.unfinished_tools(turn).count(), 0);

    // The turn waits for the prompt's answer; with no answer the fallback ends it.
    let current = rig.actor.threads[&thread].turn.current.as_ref();
    let current = current.expect("the turn is still running");
    assert!(current.phase == Phase::Prompting && current.cancelled);
    rig.clock.advance(CANCEL_WAIT);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_failed_end(&log);
    assert_eq!(updated(&log), [durable], "nothing is cancelled twice");
    rig.finish();
}

#[test]
fn a_cancel_the_failure_refused_is_logged_after_the_unblock() {
    let mut rig = Rig::start("turn-cancel-after-unblock", "");
    let (thread, _turn, durable) = failing_tool_call(&mut rig);
    // A commit behind the refused one keeps the stream blocked while the turn is cancelled.
    rig.commit(thread, vec![notice(9)]);
    let behind = |actor: &crate::actor::Actor, input: &Input| {
        matches!(input, Input::Committed { ticket, .. }
            if actor.pipeline.events(*ticket).iter().any(|e| e.event == notice(9)))
    };
    rig.run_holding("the cancel", behind, |actor| {
        let current = actor.threads[&thread].turn.current.as_ref();
        current.is_some_and(|current| current.cancelled)
    });
    assert!(
        updated(&rig.log(thread)).is_empty(),
        "the cancel was refused"
    );

    rig.release_held();
    rig.settle_commits();
    assert_eq!(updated(&rig.log(thread)), [durable]);
    let current = rig.actor.threads[&thread].turn.current.as_ref();
    assert!(current.is_some(), "the turn waits for the prompt's answer");
    rig.finish();
}

/// Ends `thread`'s running turn `EndTurn` while the store refuses `times` commits that hold a
/// `TurnEnded`.
fn end_failing(rig: &mut Rig, thread: ThreadId, times: usize) {
    rig.fail(times, |event| matches!(event, DomainEvent::TurnEnded(_)));
    let end = End {
        stop: StopReason::EndTurn,
        failure: None,
        expiry: clauseo_protocol::ExpiryReason::TurnEnded,
        detach: None,
    };
    end::end_turn(&mut rig.actor, thread, end);
    until_ended(rig, thread);
}

#[test]
fn a_failed_end_is_retried_once() {
    let mut rig = Rig::start("turn-end-retried", "");
    let thread = rig.thread();
    let _turn = rig.running_turn(thread);
    end_failing(&mut rig, thread, 1);
    let log = rig.log(thread);
    assert_failed_end(&log);
    let ends = log
        .iter()
        .filter(|e| matches!(e, DomainEvent::TurnEnded(_)));
    assert_eq!(ends.count(), 1);
    assert!(rig.actor.threads[&thread].turn.closing.is_none());
    rig.finish();
}

#[test]
fn a_retried_end_keeps_the_after_turn_checkpoint() {
    let mut rig = Rig::start("turn-end-retried-checkpoint", "");
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    rig.write_file("brief.txt", "The lease ends in May.");
    end_failing(&mut rig, thread, 1);
    let log = rig.log(thread);
    assert_failed_end(&log);
    let ended = log.iter().find_map(|event| {
        if let DomainEvent::TurnEnded(ended) = event {
            Some(*ended)
        } else {
            None
        }
    });
    let after = ended.and_then(|ended| ended.checkpoint_after);
    let recorded = rig.recorded();
    let last = recorded.last().unwrap();
    assert_eq!(after, Some(last.checkpoint()), "the step can be undone");
    assert_eq!(last.kind(), CheckpointKind::AfterTurn);
    assert_eq!(last.turn(), Some(turn));
    let changed: Vec<_> = log
        .iter()
        .filter_map(|event| {
            if let DomainEvent::FilesChanged(changed) = event {
                Some(changed.files.clone())
            } else {
                None
            }
        })
        .flatten()
        .map(|file| file.path().as_str().to_owned())
        .collect();
    assert_eq!(changed, ["brief.txt"]);
    rig.finish();
}

fn prompt(thread: ThreadId) -> HostCommand {
    HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Summarise the lease.".to_owned(),
        }],
        delivery: clauseo_protocol::DeliveryRequest::Queue,
    }
}

fn assert_turn_running(refused: &crate::error::Error) {
    assert_eq!(refused.code(), ErrorCode::ThreadTurnRunning);
    let error = HostError::from(crate::error::Error::refused(refused.code(), "x"));
    assert!(error.retryable());
}

#[test]
fn an_end_that_fails_twice_refuses_prompts_and_stays_open() {
    let mut rig = Rig::start("turn-end-twice", "");
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    end_failing(&mut rig, thread, 2);
    let log = rig.log(thread);
    assert!(
        !log.iter().any(|e| matches!(e, DomainEvent::TurnEnded(_))),
        "the open turn is left to the next start"
    );
    let closing = rig.actor.threads[&thread].turn.closing.as_ref();
    assert!(closing.is_some_and(|closing| closing.close.turn == turn && closing.retried));
    let refused = rig.dispatch(10, prompt(thread)).unwrap_err();
    assert_turn_running(&refused);
    rig.finish();
}

#[test]
fn a_prompt_is_refused_while_its_own_undo_holds_the_writer() {
    let mut rig = Rig::start("turn-own-undo", "");
    let thread = rig.thread();
    let matter = rig.matter();
    rig.actor.matters.get_mut(&matter).unwrap().turns.holder = Some(thread);
    let refused = rig.dispatch(10, prompt(thread)).unwrap_err();
    assert_turn_running(&refused);
    rig.actor.matters.get_mut(&matter).unwrap().turns.holder = None;
    rig.finish();
}

#[test]
fn an_at_limit_turn_whose_checkpoint_fails_stays_at_limit() {
    let mut rig = Rig::start("turn-at-limit", "");
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    let current = rig
        .actor
        .threads
        .get_mut(&thread)
        .unwrap()
        .turn
        .current
        .as_mut();
    let current = current.unwrap();
    current.phase = Phase::Ending;
    current.limit = Some(crate::actor::turn::LimitMark {
        window: None,
        resets_at: None,
        reason: clauseo_protocol::LimitStopReason::Temporary,
    });
    current.end = Some(End {
        stop: StopReason::EndTurn,
        failure: None,
        expiry: clauseo_protocol::ExpiryReason::TurnEnded,
        detach: None,
    });
    let failed = Err(crate::error::Error::Internal("the disk is full".to_owned()));
    end::on_after_turn(&mut rig.actor, thread, turn, failed);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    let [
        ..,
        DomainEvent::TurnFailed(_),
        DomainEvent::TurnEnded(ended),
        DomainEvent::TurnStoppedAtLimit(_),
    ] = log.as_slice()
    else {
        panic!("{:?}", log.iter().map(kind).collect::<Vec<_>>());
    };
    assert_eq!(ended.stop, StopReason::AtLimit);
    rig.finish();
}

#[test]
fn held_text_survives_a_blocked_stream() {
    let mut rig = Rig::start("turn-held-text", "");
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    let stream = StreamId::Thread(thread);
    // The first commit fails while the second is in flight: the stream stays blocked.
    rig.fail(1, |event| *event == notice(1));
    rig.commit(thread, vec![notice(1)]);
    rig.commit(thread, vec![notice(2)]);
    let behind = |actor: &crate::actor::Actor, input: &Input| {
        matches!(input, Input::Committed { ticket, .. }
            if actor.pipeline.events(*ticket).iter().any(|e| e.event == notice(2)))
    };
    rig.run_holding("the block", behind, |actor| {
        actor.pipeline.blocked(stream).is_some() && actor.work.commits == 1
    });

    let message = rig.actor.ids.mint(rig.actor.now().wall).unwrap();
    let text = "The lease ends in May.".to_owned();
    let delta = TextIn {
        turn,
        message,
        acp_message_id: None,
        kind: TextKind::Answer,
        text: text.clone(),
    };
    let now = rig.actor.now().mono;
    let state = &mut rig.actor.threads.get_mut(&thread).unwrap().turn;
    let _timers = state.text.push(now, delta);
    let mut draft = Draft::new();
    assert!(out::flush_text(&mut rig.actor, thread, &mut draft).is_err());
    assert!(draft.is_empty());
    let state = &rig.actor.threads[&thread].turn;
    assert!(state.text.has_durable(), "the text is still held");

    // After the unblock the next commit (the turn's cancel) carries it.
    rig.release_held();
    rig.settle_commits();
    let logged = rig
        .log(thread)
        .into_iter()
        .any(|event| matches!(event, DomainEvent::TextAppended(appended) if appended.text == text));
    assert!(logged);
    rig.finish();
}
