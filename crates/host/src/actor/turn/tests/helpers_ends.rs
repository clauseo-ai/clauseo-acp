//! Helpers across a run's end and a reopen (`turn/helpers.rs`): the same view after a reopen;
//! a helper a crash left open in an open turn, or past its turn, ends `Stopped` at the next
//! start, once; a helper whose engine dies mid-turn, a send-now, a failed write's retry and a
//! detach end it `Stopped`; a handback whose write fails, once or twice, is written again as it
//! was; its step and words held with no turn live land in the prompted turn that starts.

use std::cell::Cell;
use std::rc::Rc;

use futures::channel::oneshot;

use clauseo_acp::Origin;
use clauseo_protocol::{
    CommandOutcome, DeliveryRequest, DetachReason, DomainEvent, HelperOutcome, HostCommand,
    HostQuery, PageLimit, Principal, PromptBlock, QueryResult, StreamId, ThreadId, ViewEntry,
};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::helpers::{
    CANCEL, COMPLETED, DELIVERED, LAUNCH_INPUT, LEAD_STEP, PROMPT, call, in_background, lead,
    lead_step_logged, live_turn, one_turn, patch, stamped, story, words,
};
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::input::{Input, Request};
use crate::actor::turn::helpers as turn_helpers;
use crate::actor::turn::{agents, own_turn};

/// The helper's view after a reopen is the view before it: the same `Helper` and `HelperText`
/// entries, and its steps still name it.
#[test]
fn a_reopened_conversation_shows_the_same_helper() {
    let mut rig = Rig::start("helpers-reopen", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(LAUNCH_INPUT)),
    );
    in_background(&mut rig, thread, turn, "toolu_A");
    stamped(
        &mut rig,
        thread,
        at,
        "toolu_A",
        call("toolu_R1", "Read", None),
    );
    stamped(&mut rig, thread, at, "toolu_A", words("Read it."));
    let handback = call(
        "toolu_H",
        "SubagentHandback",
        Some(r#"{"message":"Digest."}"#),
    );
    stamped(&mut rig, thread, at, "toolu_A", handback);
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_H", None, COMPLETED, Some(DELIVERED)),
    );
    rig.release_held();
    until_ended(&mut rig, thread);
    let before = helper_entries(&mut rig, thread);
    assert_eq!(before.len(), 3, "{before:?}");
    let mut rig = rig.restart(2);
    let after = helper_entries(&mut rig, thread);
    assert_eq!(after, before);
    rig.finish();
}

/// A helper still open when the host stopped mid-turn (a crash or a quit: nothing ended the turn
/// and the memory of its helpers went with the run) ends `Stopped` in start-up's repair of
/// that turn, so its card does not spin after the reopen.
#[test]
fn a_helper_a_crash_left_open_is_stopped_at_the_next_start() {
    let mut rig = Rig::start("helpers-crash", "");
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    turn_helpers::start(&mut rig.actor, thread, turn, open_start()).unwrap();
    rig.settle_commits();
    let state = &mut rig.actor.threads.get_mut(&thread).unwrap().turn;
    state.current = None;
    state.helpers = None;
    let mut rig = rig.restart(1);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "start paper-reader call= model=- label=- task=true",
            "end paper-reader Stopped -",
            "turn_ended Failed",
        ]
    );
    let entries = helper_entries(&mut rig, thread);
    let [ViewEntry::Helper(entry)] = entries.as_slice() else {
        panic!("{entries:?}");
    };
    assert_eq!(entry.outcome, Some(HelperOutcome::Stopped));
    let before = rig.log(thread).len();
    let mut rig = rig.restart(2);
    helper_entries(&mut rig, thread);
    rig.settle_commits();
    assert_eq!(rig.log(thread).len(), before, "a second start adds nothing");
    rig.finish();
}

/// The engine dies mid-turn (the agent exits while its helper works): the helper ends
/// `Stopped` in the write that ends the turn, not `Failed` as when a turn ends by itself.
#[test]
fn a_helper_whose_engine_dies_mid_turn_is_stopped() {
    let launch = r#"{"update":{"update":{"_meta":{"claudeCode":{"toolName":"Agent"}},"toolCallId":"toolu_A","sessionUpdate":"tool_call","name":"Agent","status":"pending","title":"Task","kind":"think","rawInput":{"subagent_type":"paper-reader","prompt":"Read."},"content":[]}}}"#;
    let steps = format!("{PROMPT}\n{launch}\n{{\"exit\":{{\"code\":1}}}}\n");
    let mut rig = Rig::start("helpers-engine-dies", &steps);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step toolu_A -",
            "start paper-reader call=toolu_A model=haiku label=- task=true",
            "end paper-reader Stopped -",
            "turn_ended Failed",
        ]
    );
    rig.finish();
}

/// A helper the log holds open after its turn ended (the test starts it by hand once the turn is
/// over), when the run that started it crashes: the next run ends it `Stopped` once the
/// conversation loads, and a run after that adds nothing.
#[test]
fn a_helper_left_open_past_its_turn_is_stopped_once_at_the_next_load() {
    let mut rig = Rig::start("helpers-open-past-turn", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    turn_helpers::start(&mut rig.actor, thread, turn, open_start()).unwrap();
    rig.settle_commits();
    // The crash takes the run's memory of its helpers with it.
    rig.actor.threads.get_mut(&thread).unwrap().turn.helpers = None;
    let want = [
        "turn_ended EndTurn",
        "start paper-reader call= model=- label=- task=true",
        "end paper-reader Stopped -",
    ];
    let mut rig = rig.restart(1);
    // The load the page asks for ends the helper; the page after it shows it ended.
    helper_entries(&mut rig, thread);
    rig.settle_commits();
    let entries = helper_entries(&mut rig, thread);
    let [ViewEntry::Helper(entry)] = entries.as_slice() else {
        panic!("{entries:?}");
    };
    assert_eq!(entry.outcome, Some(HelperOutcome::Stopped));
    assert_eq!(story(&rig.log(thread))[1..], want);
    let mut rig = rig.restart(2);
    helper_entries(&mut rig, thread);
    rig.settle_commits();
    assert_eq!(
        story(&rig.log(thread))[1..],
        want,
        "a second start adds nothing"
    );
    rig.finish();
}

/// A Stop whose write fails: the retry after the stream drains still ends the helper, once.
#[test]
fn a_stop_whose_write_fails_ends_its_helper_on_the_retry() {
    let respond = r#"{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}"#;
    let steps = format!("{PROMPT}\n{LEAD_STEP}\n{CANCEL}\n{respond}\n");
    let mut rig = Rig::start("helpers-stop-fails", &steps);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    rig.run_until("the lead's step", |actor| lead_step_logged(actor, thread));
    let input = r#"{"subagent_type":"paper-reader","prompt":"Read."}"#;
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(input)),
    );
    in_background(&mut rig, thread, turn, "toolu_A");
    rig.fail(1, |event| matches!(event, DomainEvent::HelperEnded(_)));
    // The Stop's own write is the one refused, so its command answers with the store's error.
    let refused = rig.dispatch(11, HostCommand::Cancel { thread });
    assert!(refused.is_err(), "{refused:?}");
    until_ended(&mut rig, thread);
    let story = story(&rig.log(thread));
    let ends: Vec<&String> = story
        .iter()
        .filter(|line| line.starts_with("end "))
        .collect();
    assert_eq!(ends, ["end paper-reader Stopped -"], "{story:?}");
    rig.finish();
}

/// A message sent now while a helper runs cancels the turn, and Claude's engine stops its helpers
/// on a cancel: the helper ends `Stopped` with the cancel, and the next turn starts with none
/// open.
#[test]
fn a_send_now_stops_the_running_helper() {
    let respond = r#"{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}"#;
    let second =
        r#"{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}"#;
    let answer = r#"{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}"#;
    let steps = format!("{PROMPT}\n{LEAD_STEP}\n{CANCEL}\n{respond}\n{second}\n{answer}\n");
    let mut rig = Rig::start("helpers-send-now", &steps);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    rig.run_until("the lead's step", |actor| lead_step_logged(actor, thread));
    let input = r#"{"subagent_type":"paper-reader","prompt":"Read."}"#;
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(input)),
    );
    in_background(&mut rig, thread, turn, "toolu_A");
    let now = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Stop and read the notice instead.".to_owned(),
        }],
        delivery: DeliveryRequest::SendNow,
        config: None,
    };
    rig.dispatch(11, now).unwrap();
    rig.run_until("both turns' ends", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.queue.is_empty() && actor.work.commits == 0
    });
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step tc-0 -",
            "step toolu_A -",
            "start paper-reader call=toolu_A model=haiku label=- task=true",
            "end paper-reader Stopped -",
            "turn_ended Cancelled",
            "turn_ended EndTurn",
        ]
    );
    rig.finish();
}

/// The agent session ends with no turn running while a helper is open: the helper ends
/// `Stopped` in the detach's write.
#[test]
fn a_detach_stops_an_open_helper() {
    let mut rig = Rig::start("helpers-detach", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    turn_helpers::start(&mut rig.actor, thread, turn, open_start()).unwrap();
    rig.settle_commits();
    agents::log_detach(&mut rig.actor, thread, DetachReason::Closed);
    rig.settle_commits();
    let log = rig.log(thread);
    assert_eq!(
        story(&log)[1..],
        [
            "turn_ended EndTurn",
            "start paper-reader call= model=- label=- task=true",
            "end paper-reader Stopped -",
        ]
    );
    assert!(matches!(
        log.last(),
        Some(DomainEvent::AgentSessionDetached(_))
    ));
    rig.finish();
}

/// A helper's step and words that come with no turn live while a prompt's turn is starting wait
/// for it, and both are logged as that turn's, in the order they came, before its end.
#[test]
fn a_helper_s_held_step_and_words_land_in_the_prompted_turn() {
    let second =
        r#"{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}"#;
    let answer = r#"{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}"#;
    let steps = format!("{}{second}\n{answer}\n", one_turn("end_turn"));
    let mut rig = Rig::start("helpers-held-words", &steps);
    let thread = rig.thread();
    let first = send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    turn_helpers::start(&mut rig.actor, thread, first, open_start()).unwrap();
    rig.settle_commits();
    let prompt = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Now the notice.".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
        config: None,
    };
    // The prompt's turn waits for its before checkpoint, so no turn is live.
    let accepted = rig
        .dispatch_holding(11, prompt, |_, input| matches!(input, Input::Matter { .. }))
        .unwrap();
    let CommandOutcome::PromptAccepted { turn, .. } = accepted else {
        panic!("{accepted:?}");
    };
    let read = call("toolu_R1", "Read", None);
    stamped(&mut rig, thread, Origin::Idle, "toolu_A", read);
    stamped(&mut rig, thread, Origin::Idle, "toolu_A", words("Read it."));
    assert_eq!(own_turn::waiting(&mut rig.actor, thread), 2);
    rig.release_held();
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        story(&log)[1..],
        [
            "turn_ended EndTurn",
            "start paper-reader call= model=- label=- task=true",
            "step toolu_R1 paper-reader",
            "words paper-reader: Read it.",
            "end paper-reader Failed -",
            "turn_ended EndTurn",
        ]
    );
    let turns: Vec<bool> = log
        .iter()
        .filter_map(|event| {
            if let DomainEvent::ToolCallStarted(step) = event {
                step.helper.map(|_| step.turn == turn)
            } else if let DomainEvent::HelperText(text) = event {
                Some(text.turn == turn)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(turns, [true, true], "both are the prompted turn's");
    rig.finish();
}

/// A turn's close whose write cannot be sent at all (no blocked stream: the send itself fails)
/// with no commit in flight: the retry, sent at once, still ends the open helper, `Stopped` as
/// for any failed turn.
#[test]
fn a_close_whose_write_cannot_be_sent_ends_its_helper_on_the_retry() {
    let mut rig = Rig::start("helpers-close-unsent", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(LAUNCH_INPUT)),
    );
    in_background(&mut rig, thread, turn, "toolu_A");
    rig.release_held();
    // The after-turn checkpoint's answer, which sends the close, waits until every commit
    // before the close has landed.
    let after = |_: &Actor, input: &Input| matches!(input, Input::Matter { .. });
    rig.run_holding("the commits before the close", after, |actor| {
        actor.work.commits == 0
    });
    if rig.held.is_empty() {
        rig.hold_next("the after-turn checkpoint", after);
    }
    rig.actor.failed_sends = 1;
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step tc-0 -",
            "step toolu_A -",
            "start paper-reader call=toolu_A model=haiku label=Digest lease and notice task=true",
            "end paper-reader Stopped -",
            "turn_ended Failed",
        ]
    );
    rig.finish();
}

/// A handback whose write fails: the turn is cancelled, and once the stream drains the retry
/// writes the end that write lost, `Done` with the helper's report, not `Stopped`.
#[test]
fn a_handback_whose_write_fails_ends_its_helper_done_with_its_report_on_the_retry() {
    handback_write_fails("helpers-handback-fails", 1);
}

/// A handback whose write fails, and the retry's write too: the retry after that still writes
/// the helper's own end, `Done` with its report.
#[test]
fn a_handback_whose_write_fails_twice_still_ends_its_helper_done_with_its_report() {
    handback_write_fails("helpers-handback-fails-twice", 2);
}

/// The store refuses the first `times` writes holding a helper's end, the handback's first;
/// the helper ends once, `Done` with its report.
fn handback_write_fails(name: &str, times: usize) {
    let respond = r#"{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}"#;
    let steps = format!("{PROMPT}\n{LEAD_STEP}\n{CANCEL}\n{respond}\n");
    let mut rig = Rig::start(name, &steps);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    rig.run_until("the lead's step", |actor| lead_step_logged(actor, thread));
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(LAUNCH_INPUT)),
    );
    in_background(&mut rig, thread, turn, "toolu_A");
    let handback = call(
        "toolu_H",
        "SubagentHandback",
        Some(r#"{"message":"Digest."}"#),
    );
    stamped(&mut rig, thread, Origin::Turn(turn), "toolu_A", handback);
    // The store is asked only while refusals are left, so this counts the refused writes.
    let refused = Rc::new(Cell::new(0));
    let counted = Rc::clone(&refused);
    rig.fail(times, move |event| {
        let end = matches!(event, DomainEvent::HelperEnded(_));
        if end {
            counted.set(counted.get() + 1);
        }
        end
    });
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_H", None, COMPLETED, Some(DELIVERED)),
    );
    until_ended(&mut rig, thread);
    assert_eq!(refused.get(), times, "writes refused");
    let story = story(&rig.log(thread));
    let ends: Vec<&String> = story
        .iter()
        .filter(|line| line.starts_with("end "))
        .collect();
    assert_eq!(ends, ["end paper-reader Done Digest."], "{story:?}");
    rig.finish();
}

/// A background helper's start with no launch step, as a test puts it in by hand.
fn open_start() -> turn_helpers::Start {
    turn_helpers::Start {
        id: None,
        acp_id: "toolu_A".into(),
        parent: None,
        call: None,
        name: "paper-reader".into(),
        label: None,
        task: Some("Read.".to_owned()),
        model: None,
        background: true,
    }
}

/// The thread's view entries that belong to a helper (its card, its words, its steps), as a
/// `PageEntries` query answers them: the view a reopened conversation shows.
fn helper_entries(rig: &mut Rig, thread: ThreadId) -> Vec<ViewEntry> {
    let (reply, mut answer) = oneshot::channel();
    let query = HostQuery::PageEntries {
        stream: StreamId::Thread(thread),
        before_order: None,
        limit: PageLimit::new(200).unwrap(),
    };
    let principal = Principal::LocalDesktop;
    rig.actor.on_request(Request::Query {
        principal,
        query,
        reply,
    });
    let mut got = None;
    rig.run_until("the page", |_| {
        if got.is_none()
            && let Ok(Some(result)) = answer.try_recv()
        {
            got = Some(result);
        }
        got.is_some()
    });
    let Some(Ok(QueryResult::Entries(entries))) = got else {
        panic!("{got:?}");
    };
    entries
        .into_iter()
        .filter(|entry| {
            matches!(entry, ViewEntry::Helper(_) | ViewEntry::HelperText(_))
                || matches!(entry, ViewEntry::ToolCall(call) if call.helper.is_some())
        })
        .collect()
}
