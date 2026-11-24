//! Timing windows of turns that run beside each other in one matter:
//!
//! - a turn whose after-turn checkpoint fails has its papers the checkpoints of a turn beside
//!   it took (its before-turn checkpoint, or its after-turn one when it ended first) logged as
//!   outside edits on them;
//! - the other turns' places are read when the matter's thread runs a checkpoint, not when the
//!   job was sent;
//! - a write call another answer made that was refused does not name it on a card;
//! - a paper a failed turn's end logged as an outside edit is off the cards of the turns still
//!   running beside it while it holds what was logged;
//! - those outside edits keep the ones the lawyer made, already logged on that checkpoint, even
//!   when the end waited for a limits read while another answer's checkpoint took them;
//! - a paper no answer named that changes while two run is on both cards, flagged (ADR 0011).

use clauseo_protocol::{
    AgentChoiceId, CheckpointId, DomainEvent, HostCommand, MatterPath, StreamId, ThreadId, TurnId,
    ViewEntry,
};
use clauseo_reducer::StreamState;

use super::rig::Rig;
use super::together_order::{WRITES, card, prompt_answer_of, send_prompt};
use crate::actor::input::{Input, MatterTag};
use crate::actor::turn::{JobKind, TurnJob, TurnPhase, end};
use crate::error::Error;
use crate::matter_worker::{MatterReply, hook};

/// "Write A." writes notes/a.txt with a completed edit call. "Edit A too." starts an edit call
/// naming notes/a.txt, asks perm-b and, allowed, writes notes/a.txt and completes the call.
const SAME_PAPER_ALLOWED: &str = r#"{"one_of":[[{"expect":{"method":"session/prompt","as":"pa","params":{"sessionId":"sess-1","prompt":[{"type":"text","text":"Write A."}]}}},{"write_file":{"path":"notes/a.txt","content":"Alpha\n"}},{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"t-a","title":"Write notes/a.txt","kind":"edit","status":"completed","locations":[{"path":"{{cwd}}/notes/a.txt"}]}}},{"respond":{"to":"pa","result":{"stopReason":"end_turn"}}}],[{"expect":{"method":"session/prompt","as":"pb","params":{"sessionId":"sess-1","prompt":[{"type":"text","text":"Edit A too."}]}}},{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"t-b","title":"Edit notes/a.txt","kind":"edit","status":"pending","locations":[{"path":"{{cwd}}/notes/a.txt"}]}}},{"request":{"id":"perm-b","method":"session/request_permission","params":{"sessionId":"sess-1","toolCall":{"toolCallId":"t-b","title":"Edit notes/a.txt","kind":"edit","status":"pending"},"options":[{"optionId":"allow-once","name":"Allow once","kind":"allow_once"},{"optionId":"reject-once","name":"Reject","kind":"reject_once"}]}}},{"expect_response":{"id":"perm-b","result":{"outcome":{"outcome":"selected","optionId":"allow-once"}}}},{"write_file":{"path":"notes/a.txt","content":"Alpha\nBeta\n"}},{"update":{"update":{"sessionUpdate":"tool_call_update","toolCallId":"t-b","status":"completed"}}},{"respond":{"to":"pb","result":{"stopReason":"end_turn"}}}]]}
"#;

/// The same, the lawyer refusing perm-b: the call fails and writes nothing.
const SAME_PAPER_REFUSED: &str = r#"{"one_of":[[{"expect":{"method":"session/prompt","as":"pa","params":{"sessionId":"sess-1","prompt":[{"type":"text","text":"Write A."}]}}},{"write_file":{"path":"notes/a.txt","content":"Alpha\n"}},{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"t-a","title":"Write notes/a.txt","kind":"edit","status":"completed","locations":[{"path":"{{cwd}}/notes/a.txt"}]}}},{"respond":{"to":"pa","result":{"stopReason":"end_turn"}}}],[{"expect":{"method":"session/prompt","as":"pb","params":{"sessionId":"sess-1","prompt":[{"type":"text","text":"Edit A too."}]}}},{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"t-b","title":"Edit notes/a.txt","kind":"edit","status":"pending","locations":[{"path":"{{cwd}}/notes/a.txt"}]}}},{"request":{"id":"perm-b","method":"session/request_permission","params":{"sessionId":"sess-1","toolCall":{"toolCallId":"t-b","title":"Edit notes/a.txt","kind":"edit","status":"pending"},"options":[{"optionId":"allow-once","name":"Allow once","kind":"allow_once"},{"optionId":"reject-once","name":"Reject","kind":"reject_once"}]}}},{"expect_response":{"id":"perm-b","result":{"outcome":{"outcome":"selected","optionId":"reject-once"}}}},{"update":{"update":{"sessionUpdate":"tool_call_update","toolCallId":"t-b","status":"failed"}}},{"respond":{"to":"pb","result":{"stopReason":"end_turn"}}}]]}
"#;

/// "Ask, then write A." asks perm-a for an edit call naming notes/a.txt and, allowed, writes it
/// and completes the call; "Write B." writes notes/b.txt with a completed edit call.
pub(super) const ASK_THEN_A: &str = r#"{"one_of":[[{"expect":{"method":"session/prompt","as":"pa","params":{"sessionId":"sess-1","prompt":[{"type":"text","text":"Ask, then write A."}]}}},{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"t-a","title":"Write notes/a.txt","kind":"edit","status":"pending","locations":[{"path":"{{cwd}}/notes/a.txt"}]}}},{"request":{"id":"perm-a","method":"session/request_permission","params":{"sessionId":"sess-1","toolCall":{"toolCallId":"t-a","title":"Write notes/a.txt","kind":"edit","status":"pending"},"options":[{"optionId":"allow-once","name":"Allow once","kind":"allow_once"}]}}},{"expect_response":{"id":"perm-a","result":{"outcome":{"outcome":"selected","optionId":"allow-once"}}}},{"write_file":{"path":"notes/a.txt","content":"Alpha\n"}},{"update":{"update":{"sessionUpdate":"tool_call_update","toolCallId":"t-a","status":"completed"}}},{"respond":{"to":"pa","result":{"stopReason":"end_turn"}}}],[{"expect":{"method":"session/prompt","as":"pb","params":{"sessionId":"sess-1","prompt":[{"type":"text","text":"Write B."}]}}},{"write_file":{"path":"notes/b.txt","content":"Beta\n"}},{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"t-b","title":"Write notes/b.txt","kind":"edit","status":"completed","locations":[{"path":"{{cwd}}/notes/b.txt"}]}}},{"respond":{"to":"pb","result":{"stopReason":"end_turn"}}}]]}
"#;

/// `ASK_THEN_A` with "Write C.", which writes notes/c.txt with a completed edit call.
const ASK_THEN_A_AND_C: &str = r#"{"one_of":[[{"expect":{"method":"session/prompt","as":"pa","params":{"sessionId":"sess-1","prompt":[{"type":"text","text":"Ask, then write A."}]}}},{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"t-a","title":"Write notes/a.txt","kind":"edit","status":"pending","locations":[{"path":"{{cwd}}/notes/a.txt"}]}}},{"request":{"id":"perm-a","method":"session/request_permission","params":{"sessionId":"sess-1","toolCall":{"toolCallId":"t-a","title":"Write notes/a.txt","kind":"edit","status":"pending"},"options":[{"optionId":"allow-once","name":"Allow once","kind":"allow_once"}]}}},{"expect_response":{"id":"perm-a","result":{"outcome":{"outcome":"selected","optionId":"allow-once"}}}},{"write_file":{"path":"notes/a.txt","content":"Alpha\n"}},{"update":{"update":{"sessionUpdate":"tool_call_update","toolCallId":"t-a","status":"completed"}}},{"respond":{"to":"pa","result":{"stopReason":"end_turn"}}}],[{"expect":{"method":"session/prompt","as":"pb","params":{"sessionId":"sess-1","prompt":[{"type":"text","text":"Write B."}]}}},{"write_file":{"path":"notes/b.txt","content":"Beta\n"}},{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"t-b","title":"Write notes/b.txt","kind":"edit","status":"completed","locations":[{"path":"{{cwd}}/notes/b.txt"}]}}},{"respond":{"to":"pb","result":{"stopReason":"end_turn"}}}],[{"expect":{"method":"session/prompt","as":"pc","params":{"sessionId":"sess-1","prompt":[{"type":"text","text":"Write C."}]}}},{"write_file":{"path":"notes/c.txt","content":"Gamma\n"}},{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"t-c","title":"Write notes/c.txt","kind":"edit","status":"completed","locations":[{"path":"{{cwd}}/notes/c.txt"}]}}},{"respond":{"to":"pc","result":{"stopReason":"end_turn"}}}]]}
"#;

/// Whether `input` answers an after-turn checkpoint.
fn after_turn_answer(input: &Input) -> bool {
    matches!(
        input,
        Input::Matter {
            tag: MatterTag::Turn(TurnJob {
                kind: JobKind::After(_),
                ..
            }),
            ..
        }
    )
}

/// Runs until `thread`'s turn has ended and nothing is in flight.
pub(super) fn until_ended(rig: &mut Rig, threads: &[ThreadId]) {
    rig.run_until("the turns ended", |actor| {
        threads.iter().all(|thread| {
            let state = &actor.threads[thread].turn;
            state.current.is_none() && state.closing.is_none()
        }) && actor.work.commits == 0
            && actor.work.matter_jobs == 0
    });
}

/// Runs until `thread`'s turn waits on a permission, and returns the request.
pub(super) fn until_asked(
    rig: &mut Rig,
    thread: ThreadId,
) -> clauseo_protocol::PermissionRequestId {
    rig.run_until("the permission request", |actor| {
        actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .is_some_and(|agent| !agent.permissions.is_empty())
    });
    let agent = rig.actor.threads[&thread].turn.agent.as_deref().unwrap();
    *agent.permissions.keys().next().unwrap()
}

/// The answer `option` to `request` of `thread`.
pub(super) fn answer(
    rig: &mut Rig,
    n: u128,
    thread: ThreadId,
    request: clauseo_protocol::PermissionRequestId,
    option: &str,
) {
    let answer = HostCommand::AnswerPermission {
        thread,
        request,
        option_id: AgentChoiceId::new(option).unwrap(),
    };
    rig.dispatch(n, answer).unwrap();
}

/// The `checkpoint_after` of `turn` in `log`.
fn after_of(log: &[DomainEvent], turn: TurnId) -> Option<CheckpointId> {
    log.iter().find_map(|event| {
        let DomainEvent::TurnEnded(ended) = event else {
            return None;
        };
        ended.checkpoint_after.filter(|_| ended.turn == turn)
    })
}

/// B's after-turn checkpoint fails; B has no card.
fn fail_b(rig: &mut Rig, second: ThreadId, turn_b: TurnId) {
    rig.release_held();
    rig.hold_next("B's after-turn checkpoint", |_, input| {
        after_turn_answer(input)
    });
    let Some(Input::Matter { matter, tag, .. }) = rig.held.pop() else {
        panic!("B's after-turn checkpoint is not held");
    };
    let failed = Err(Error::Internal("a test fails this checkpoint".to_owned()));
    rig.actor.on_input(Input::Matter {
        matter,
        tag,
        reply: MatterReply::AfterTurn(failed),
    });
    until_ended(rig, &[second]);
    let log_b = rig.log(second);
    let carded = log_b
        .iter()
        .any(|event| matches!(event, DomainEvent::FilesChanged(changed) if changed.turn == turn_b));
    assert!(!carded, "B has a card");
}

/// The matter's outside edits: each checkpoint with its papers, and whether every file is plain
/// (neither flagged nor naming anyone).
fn outside_edits(rig: &mut Rig) -> Vec<(CheckpointId, Vec<String>, bool)> {
    rig.matter_log()
        .iter()
        .filter_map(|event| {
            let DomainEvent::OutsideEdits(edits) = event else {
                return None;
            };
            let paths = edits.files.iter().map(|f| f.path().as_str().to_owned());
            let plain = edits
                .files
                .iter()
                .all(|f| !f.may_include_lawyer_edits() && f.also_changed_by().is_empty());
            Some((edits.checkpoint, paths.collect(), plain))
        })
        .collect()
}

#[test]
fn a_failed_answers_papers_an_earlier_after_turn_checkpoint_took_are_outside_edits() {
    let mut rig = Rig::start("together-absorbed-after", ASK_THEN_A);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    // A starts and waits on the lawyer.
    let turn_a = send_prompt(&mut rig, 10, first, "Ask, then write A.");
    let request = until_asked(&mut rig, first);
    // B starts beside A and writes notes/b.txt; its prompt's answer is held, so B runs on.
    let turn_b = send_prompt(&mut rig, 11, second, "Write B.");
    rig.hold_next("B's prompt answer", |_, input| {
        prompt_answer_of(second, input)
    });
    // A writes notes/a.txt and ends: its after-turn checkpoint holds b.txt, left off its card.
    answer(&mut rig, 12, first, request, "allow-once");
    until_ended(&mut rig, &[first]);
    let log_a = rig.log(first);
    assert_eq!(
        card(&log_a, turn_a),
        [("notes/a.txt".to_owned(), false, vec![])]
    );
    let a_after = after_of(&log_a, turn_a).expect("A's checkpoint");
    assert_eq!(outside_edits(&mut rig), []);

    // B's after-turn checkpoint fails: b.txt is logged as an outside edit on A's.
    fail_b(&mut rig, second, turn_b);
    let b_txt = vec!["notes/b.txt".to_owned()];
    assert_eq!(outside_edits(&mut rig), [(a_after, b_txt, true)]);
    rig.finish();
}

#[test]
fn a_failed_answers_papers_a_later_before_turn_checkpoint_took_are_outside_edits() {
    let mut rig = Rig::start("together-absorbed-before", WRITES);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    // B writes notes/b.txt with a completed call; its prompt's answer is held, so B runs on.
    let turn_b = send_prompt(&mut rig, 10, second, "Write B.");
    rig.hold_next("B's prompt answer", |_, input| {
        prompt_answer_of(second, input)
    });
    // A starts beside B: its before-turn checkpoint holds b.txt, left out of its outside edits.
    let turn_a = send_prompt(&mut rig, 11, first, "Write A.");
    until_ended(&mut rig, &[first]);
    let log_a = rig.log(first);
    assert_eq!(
        card(&log_a, turn_a),
        [("notes/a.txt".to_owned(), false, vec![])]
    );
    let a_before = log_a
        .iter()
        .find_map(|event| {
            let DomainEvent::TurnStarted(started) = event else {
                return None;
            };
            (started.turn == turn_a).then_some(started.checkpoint_before)
        })
        .expect("A started");
    assert_eq!(outside_edits(&mut rig), []);

    // B's after-turn checkpoint fails: b.txt is logged as an outside edit on A's start.
    fail_b(&mut rig, second, turn_b);
    let b_txt = vec!["notes/b.txt".to_owned()];
    assert_eq!(outside_edits(&mut rig), [(a_before, b_txt, true)]);
    rig.finish();
}

#[test]
fn a_failed_answers_outside_edits_keep_the_lawyers_on_that_version() {
    let mut rig = Rig::start("together-absorbed-kept", WRITES);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    // The folder changed, so B's before-turn checkpoint is a new one: nothing reads the
    // matter's log for it.
    rig.write_file("brief.txt", "The lease ends in May.");
    // B writes notes/b.txt; its prompt's answer is held, so B runs on.
    let turn_b = send_prompt(&mut rig, 10, second, "Write B.");
    rig.hold_next("B's prompt answer", |_, input| {
        prompt_answer_of(second, input)
    });
    // The lawyer edits memo.txt; A starts beside B, so its before-turn checkpoint logs memo.txt
    // as an outside edit, b.txt left out as B's.
    rig.write_file("memo.txt", "Call the landlord.");
    let turn_a = send_prompt(&mut rig, 11, first, "Write A.");
    until_ended(&mut rig, &[first]);
    let a_before = rig
        .log(first)
        .iter()
        .find_map(|event| {
            let DomainEvent::TurnStarted(started) = event else {
                return None;
            };
            (started.turn == turn_a).then_some(started.checkpoint_before)
        })
        .expect("A started");
    assert_eq!(version_files(&mut rig, a_before), ["memo.txt"]);

    // B's after-turn checkpoint fails: b.txt joins memo.txt on A's start, the lawyer's edit kept.
    fail_b(&mut rig, second, turn_b);
    assert_eq!(
        version_files(&mut rig, a_before),
        ["memo.txt", "notes/b.txt"]
    );
    rig.finish();
}

/// Whether `input` answers a `kind` checkpoint of `thread`'s turn.
fn checkpoint_answer_of(thread: ThreadId, before: bool, input: &Input) -> bool {
    let Input::Matter {
        tag: MatterTag::Turn(job),
        ..
    } = input
    else {
        return false;
    };
    job.thread == thread && matches!(job.kind, JobKind::Before(_)) == before
}

#[test]
fn outside_edits_logged_after_a_limits_read_keep_the_lawyers_on_that_version() {
    let mut rig = Rig::start("together-absorbed-read", WRITES);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    rig.write_file("brief.txt", "The lease ends in May.");
    // B writes notes/b.txt; its prompt's answer is held, so B runs on.
    let turn_b = send_prompt(&mut rig, 10, second, "Write B.");
    rig.hold_next("B's prompt answer", |_, input| {
        prompt_answer_of(second, input)
    });
    let prompt_b = rig.held.pop().expect("B's prompt answer");
    // The lawyer edits memo.txt; A starts beside B. Its before-turn checkpoint (taken before B's
    // after-turn one) holds b.txt and memo.txt; its answer is held.
    rig.write_file("memo.txt", "Call the landlord.");
    let start_a = HostCommand::SendPrompt {
        thread: first,
        content: vec![clauseo_protocol::PromptBlock::Text {
            text: "Write A.".to_owned(),
        }],
        delivery: clauseo_protocol::DeliveryRequest::Queue,
    };
    rig.dispatch_holding(11, start_a, |_, input| {
        checkpoint_answer_of(first, true, input)
    })
    .unwrap();
    if rig.held.is_empty() {
        rig.hold_next("A's before-turn checkpoint", |_, input| {
            checkpoint_answer_of(first, true, input)
        });
    }
    let before_a = rig.held.pop().expect("A's before-turn answer");
    // B's end waits for a limits read (the Codex fallback). Its after-turn checkpoint fails and
    // the answer is held for the read; nothing is absorbed yet, so no log is read for it.
    let current = rig.actor.threads.get_mut(&second).unwrap();
    current.turn.current.as_mut().unwrap().limit_read = Some(end::LimitRead::default());
    rig.actor.on_input(prompt_b);
    rig.hold_next("B's after-turn checkpoint", |_, input| {
        checkpoint_answer_of(second, false, input)
    });
    let Some(Input::Matter { matter, tag, .. }) = rig.held.pop() else {
        panic!("B's after-turn checkpoint is not held");
    };
    let failed = Err(Error::Internal("a test fails this checkpoint".to_owned()));
    rig.actor.on_input(Input::Matter {
        matter,
        tag,
        reply: MatterReply::AfterTurn(failed),
    });
    rig.run_until("B's answer held for its read", |actor| {
        let current = actor.threads[&second].turn.current.as_deref();
        let read = current.and_then(|current| current.limit_read.as_ref());
        read.is_some_and(end::LimitRead::holds_checkpoint)
    });
    // A's start logs memo.txt as an outside edit on its before-turn checkpoint and leaves b.txt
    // to B, absorbed: B's to log should it end with no card.
    rig.actor.on_input(before_a);
    until_ended(&mut rig, &[first]);
    let a_before = rig
        .log(first)
        .iter()
        .find_map(|event| {
            let DomainEvent::TurnStarted(started) = event else {
                return None;
            };
            (started.turn != turn_b).then_some(started.checkpoint_before)
        })
        .expect("A started");
    assert_eq!(version_files(&mut rig, a_before), ["memo.txt"]);

    // The read is over: B ends with no card, b.txt joins memo.txt on A's start.
    end::on_limit_read_timer(&mut rig.actor, second, turn_b);
    until_ended(&mut rig, &[second]);
    assert_eq!(
        version_files(&mut rig, a_before),
        ["memo.txt", "notes/b.txt"]
    );
    rig.finish();
}

#[test]
fn a_paper_no_answer_named_is_flagged_on_both_cards() {
    let mut rig = Rig::start("together-unnamed", ASK_THEN_A);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    // A starts and waits on the lawyer; B starts beside it and writes notes/b.txt, running on.
    let turn_a = send_prompt(&mut rig, 10, first, "Ask, then write A.");
    let request = until_asked(&mut rig, first);
    let turn_b = send_prompt(&mut rig, 11, second, "Write B.");
    rig.hold_next("B's prompt answer", |_, input| {
        prompt_answer_of(second, input)
    });
    // memo.txt changes while both run, and no tool call names it.
    rig.write_file("memo.txt", "Call the landlord.");
    answer(&mut rig, 12, first, request, "allow-once");
    until_ended(&mut rig, &[first]);
    rig.release_held();
    until_ended(&mut rig, &[second]);
    // Clauseo cannot tell which answer changed it: it is on both cards, flagged, naming no one.
    let memo = ("memo.txt".to_owned(), true, vec![]);
    assert_eq!(
        card(&rig.log(first), turn_a),
        [memo.clone(), ("notes/a.txt".to_owned(), false, vec![])]
    );
    assert_eq!(
        card(&rig.log(second), turn_b),
        [memo, ("notes/b.txt".to_owned(), false, vec![])]
    );
    rig.finish();
}

/// The files the matter's versions list shows on `checkpoint`, folded from its whole log.
fn version_files(rig: &mut Rig, checkpoint: CheckpointId) -> Vec<String> {
    let stream = StreamId::Matter(rig.matter());
    let store = rig.actor.store.clone();
    let reopen = rig
        .run(crate::actor::streams::read_whole_log(&store, stream))
        .unwrap();
    let mut state = StreamState::new(stream);
    for envelope in &reopen.tail {
        clauseo_reducer::apply(&mut state, envelope).unwrap();
    }
    let matter = state.as_matter().expect("a matter's state");
    let files = matter.entries().values().find_map(|entry| {
        let ViewEntry::Version(version) = entry else {
            return None;
        };
        (version.checkpoint == checkpoint).then(|| version.files.clone())
    });
    let files = files.unwrap_or_default();
    files.iter().map(|f| f.path().as_str().to_owned()).collect()
}

#[test]
fn a_write_that_starts_after_the_job_was_sent_names_its_answer_on_the_card() {
    let mut rig = Rig::start("together-at-run", SAME_PAPER_ALLOWED);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    // B's call naming notes/a.txt waits on the lawyer.
    send_prompt(&mut rig, 10, second, "Edit A too.");
    let request = until_asked(&mut rig, second);
    // A writes notes/a.txt and ends; its after-turn checkpoint is sent and held at its start
    // on the matter's thread.
    let root = rig.actor.effective.data_dir().join("matters");
    let gate = hook::gate_next(&root, "AfterTurn");
    let turn_a = send_prompt(&mut rig, 11, first, "Write A.");
    rig.run_until("A's after-turn checkpoint waits", |_| gate.reached());
    // Only now does B's write run and its call complete.
    answer(&mut rig, 12, second, request, "allow-once");
    rig.run_until("B's call completed", |actor| {
        let current = actor.threads[&second].turn.current.as_deref();
        current.is_none_or(|current| current.phase == TurnPhase::Ending)
    });
    gate.open();
    until_ended(&mut rig, &[first, second]);
    // B changed notes/a.txt before A's checkpoint ran: A's card names it.
    let log_a = rig.log(first);
    assert_eq!(
        card(&log_a, turn_a),
        [("notes/a.txt".to_owned(), false, vec![second])]
    );
    rig.finish();
}

#[test]
fn a_refused_write_call_of_another_answer_does_not_name_it_on_the_card() {
    let mut rig = Rig::start("together-refused", SAME_PAPER_REFUSED);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    // B's call naming notes/a.txt is refused; B's prompt's answer is held, so B runs on.
    send_prompt(&mut rig, 10, second, "Edit A too.");
    let request = until_asked(&mut rig, second);
    answer(&mut rig, 11, second, request, "reject-once");
    rig.hold_next("B's prompt answer", |_, input| {
        prompt_answer_of(second, input)
    });
    // A writes notes/a.txt and ends beside B.
    let turn_a = send_prompt(&mut rig, 12, first, "Write A.");
    until_ended(&mut rig, &[first]);
    let log_a = rig.log(first);
    assert_eq!(
        card(&log_a, turn_a),
        [("notes/a.txt".to_owned(), false, vec![])]
    );
    let beside: Vec<MatterPath> = log_a
        .iter()
        .find_map(|event| {
            let DomainEvent::FilesChanged(changed) = event else {
                return None;
            };
            (changed.turn == turn_a).then(|| changed.beside.clone())
        })
        .unwrap_or_default();
    assert!(beside.is_empty(), "{beside:?}");
    rig.release_held();
    until_ended(&mut rig, &[second]);
    rig.finish();
}

#[test]
fn a_failed_answers_logged_paper_is_off_the_card_of_the_answer_still_running() {
    let mut rig = Rig::start("together-absorbed-running", ASK_THEN_A_AND_C);
    let (c, b, a) = (rig.thread(), rig.thread_by(2), rig.thread_by(3));
    // C starts and waits on the lawyer.
    let turn_c = send_prompt(&mut rig, 10, c, "Ask, then write A.");
    let request = until_asked(&mut rig, c);
    // B starts beside C and writes notes/b.txt; its prompt's answer is held, so B runs on.
    let turn_b = send_prompt(&mut rig, 11, b, "Write B.");
    rig.hold_next("B's prompt answer", |_, input| prompt_answer_of(b, input));
    // A writes notes/c.txt and ends beside both: its checkpoints hold b.txt, left out as B's.
    let turn_a = send_prompt(&mut rig, 12, a, "Write C.");
    until_ended(&mut rig, &[a]);
    let log_a = rig.log(a);
    assert_eq!(
        card(&log_a, turn_a),
        [("notes/c.txt".to_owned(), false, vec![])]
    );

    // B's after-turn checkpoint fails: b.txt is logged as an outside edit on A's start.
    fail_b(&mut rig, b, turn_b);
    let logged = outside_edits(&mut rig);
    let [(_, paths, true)] = logged.as_slice() else {
        panic!("{logged:?}");
    };
    assert_eq!(paths, &["notes/b.txt".to_owned()]);

    // C writes notes/a.txt and ends: b.txt, as logged still, is not on its card too.
    answer(&mut rig, 13, c, request, "allow-once");
    until_ended(&mut rig, &[c]);
    let log_c = rig.log(c);
    assert_eq!(
        card(&log_c, turn_c),
        [("notes/a.txt".to_owned(), false, vec![])]
    );
    assert_eq!(outside_edits(&mut rig), logged, "b.txt is recorded once");
    rig.finish();
}
