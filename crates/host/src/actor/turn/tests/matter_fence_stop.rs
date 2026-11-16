//! A read taken away while a turn runs (`idle/fence.rs`): withdrawing an invite, or forgetting
//! the invited matter, stops the running turn at once by the lawyer's Stop (`session/cancel`),
//! logs one `MatterReadStopped` notice, and the next prompt runs on a relaunch whose rules deny
//! that matter again. An invite while a turn runs stops nothing; it reaches the next launch.

use clauseo_protocol::{
    DeliveryRequest, DomainEvent, HostCommand, MatterId, NoticeKind, PromptBlock, StopReason,
    ThreadId,
};

use super::idle::{TWO_ANSWERS, rig};
use super::matter_fence::{other_matter, rules_for, turn};
use super::rig::Rig;
use crate::pipeline::tests::support::TempDir;

/// The prompt the agent holds until it is cancelled.
const HANG: &str = "Hang on.";

/// A prompt slot: [`HANG`] answers a first word, then waits for `session/cancel` and ends
/// `cancelled`; any other prompt is answered and ends `end_turn`.
fn slot(n: usize) -> String {
    format!(
        r#"{{"one_of":[[{{"expect":{{"method":"session/prompt","as":"h{n}","params":{{"sessionId":"sess-1","prompt":[{{"type":"text","text":"{HANG}"}}]}}}}}},{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","messageId":"m-h{n}","content":{{"type":"text","text":"Reading."}}}}}}}},{{"expect":{{"method":"session/cancel","params":{{"sessionId":"sess-1"}}}}}},{{"respond":{{"to":"h{n}","result":{{"stopReason":"cancelled"}}}}}}],[{{"expect":{{"method":"session/prompt","as":"p{n}","params":{{"sessionId":"sess-1"}}}}}},{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","messageId":"m-p{n}","content":{{"type":"text","text":"Done."}}}}}}}},{{"respond":{{"to":"p{n}","result":{{"stopReason":"end_turn"}}}}}}]]}}"#
    )
}

/// Three prompt slots.
fn steps() -> String {
    (0..3).map(|n| slot(n) + "\n").collect()
}

/// A rig with the other matter Smith invited into its thread before any launch.
fn invited(test: &str, papers: &TempDir) -> (Rig, ThreadId, MatterId) {
    let mut rig = rig(test, &steps(), "exit");
    let thread = rig.thread();
    let other = other_matter(&mut rig, papers.path(), "Smith", 5);
    let allow = HostCommand::AllowMatterRead {
        thread,
        matter: other,
    };
    rig.dispatch(6, allow).unwrap();
    (rig, thread, other)
}

/// Sends [`HANG`] as command `n` and runs until the agent's first words are in: the turn is
/// prompting on a launch that reads Smith.
fn hanging(rig: &mut Rig, thread: ThreadId, n: u128) {
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: HANG.to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
    };
    rig.dispatch(n, command).unwrap();
    rig.run_until("the answer's first words", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.first_words.is_some())
    });
}

/// Runs until the stopped turn has ended and its stale agent is gone.
fn until_stopped(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("the turn's end and the agent's stop", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none()
            && state.closing.is_none()
            && state.agent.is_none()
            && actor.work.commits == 0
    });
}

/// The stops of `thread`'s ended turns, in order, and its `MatterReadStopped` notices' matters.
fn stops_and_notices(rig: &mut Rig, thread: ThreadId) -> (Vec<StopReason>, Vec<MatterId>) {
    let log = rig.log(thread);
    let mut stops = Vec::new();
    let mut notices = Vec::new();
    for event in &log {
        if let DomainEvent::TurnEnded(ended) = event {
            stops.push(ended.stop);
        }
        if let DomainEvent::Notice(notice) = event
            && let Some(NoticeKind::MatterReadStopped { matter, name }) = &notice.kind
        {
            assert_eq!(name, "Smith");
            notices.push(*matter);
        }
    }
    (stops, notices)
}

/// A withdrawal while the turn runs cancels it at once, with one notice naming the matter; the
/// next prompt runs on a relaunch that denies Smith's files again, so a later Read is refused.
#[test]
fn a_withdrawal_mid_turn_stops_the_turn_and_the_relaunch_refuses_the_matter() {
    let papers = TempDir::new("mfence-stop-withdraw-papers");
    let (mut rig, thread, other) = invited("mfence-stop-withdraw", &papers);
    hanging(&mut rig, thread, 10);
    assert_eq!(
        rules_for(&rig, thread, other),
        (false, true, true),
        "invited"
    );
    let withdraw = HostCommand::WithdrawMatterRead {
        thread,
        matter: other,
    };
    rig.dispatch(11, withdraw).unwrap();
    until_stopped(&mut rig, thread);
    assert_eq!(
        stops_and_notices(&mut rig, thread),
        (vec![StopReason::Cancelled], vec![other])
    );
    turn(&mut rig, thread, 12);
    assert_eq!(
        rules_for(&rig, thread, other),
        (true, true, false),
        "fenced again"
    );
    let (stops, notices) = stops_and_notices(&mut rig, thread);
    assert_eq!(stops, vec![StopReason::Cancelled, StopReason::EndTurn]);
    assert_eq!(notices.len(), 1, "one notice");
    rig.finish();
}

/// Forgetting the invited matter while the turn runs stops it the same way.
#[test]
fn forgetting_an_invited_matter_mid_turn_stops_the_turn() {
    let papers = TempDir::new("mfence-stop-forget-papers");
    let (mut rig, thread, other) = invited("mfence-stop-forget", &papers);
    hanging(&mut rig, thread, 10);
    rig.dispatch(11, HostCommand::ForgetMatter { matter: other })
        .unwrap();
    until_stopped(&mut rig, thread);
    assert_eq!(
        stops_and_notices(&mut rig, thread),
        (vec![StopReason::Cancelled], vec![other])
    );
    turn(&mut rig, thread, 12);
    assert_eq!(
        rules_for(&rig, thread, other),
        (true, true, false),
        "fenced again"
    );
    rig.finish();
}

/// An invite while a turn runs stops nothing: the turn ends `end_turn`, with no notice, and the
/// next prompt's relaunch reads the matter.
#[test]
fn an_invite_mid_turn_waits_for_the_next_prompt() {
    let mut rig = rig("mfence-stop-invite", TWO_ANSWERS, "exit");
    let papers = TempDir::new("mfence-stop-invite-papers");
    let thread = rig.thread();
    let other = other_matter(&mut rig, papers.path(), "Smith", 5);
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "The lease?".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
    };
    let held = |_: &crate::actor::Actor, input: &crate::actor::input::Input| {
        matches!(input, crate::actor::input::Input::Agent { .. })
    };
    rig.dispatch_holding(10, command, held).unwrap();
    let allow = HostCommand::AllowMatterRead {
        thread,
        matter: other,
    };
    rig.dispatch_holding(11, allow, held).unwrap();
    assert!(
        rig.actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| !current.cancelled),
        "the turn runs on"
    );
    rig.release_held();
    rig.run_until("the turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.closing.is_none() && actor.work.commits == 0
    });
    let (stops, notices) = stops_and_notices(&mut rig, thread);
    assert_eq!((stops, notices), (vec![StopReason::EndTurn], vec![]));
    turn(&mut rig, thread, 12);
    assert_eq!(
        rules_for(&rig, thread, other),
        (false, true, true),
        "read on the relaunch"
    );
    rig.finish();
}
