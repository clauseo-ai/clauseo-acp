//! A Case Law call the relay refused for lack of credit marks its session's thread
//! (`case_law.rs`): one `CaseLawOutOfCredit` notice a turn, none in another thread, the same for
//! a helper's call through its lead's session, and nothing for a stale session or for the
//! relay's other events. The relay's events are put in by hand, as the relay posts them.

use clauseo_acp::{AcpEvent, Origin};
use clauseo_connectors::PaymentRef;
use clauseo_protocol::{AgentSessionId, CompactString, DomainEvent, NoticeKind, ThreadId};

use super::agents::send_prompt;
use super::idle::{TWO_ANSWERS, rig, until_idle};
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::connectors::ConnectorInput;
use crate::actor::input::Input;
use crate::actor::turn::{AgentReply, TurnInput};
use crate::agent::AgentInput;
use crate::config::SessionKey;
use crate::connectors::{CredentialEnd, RelayEvent};

fn relay(event: RelayEvent) -> Input {
    Input::Connector(Box::new(ConnectorInput::Relay(event)))
}

/// What the relay posts when a call of `session` comes back out of credit.
pub(super) fn refused(session: SessionKey) -> Input {
    relay(RelayEvent::OutOfCredit {
        session,
        payment: PaymentRef::default(),
    })
}

fn prompt_answer(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Turn(TurnInput::Agent {
            reply: AgentReply::Prompt { .. },
            ..
        })
    )
}

/// `thread`'s current session.
pub(super) fn key(rig: &Rig, thread: ThreadId) -> SessionKey {
    rig.actor.threads[&thread].turn.run.as_ref().unwrap().key
}

/// How many `CaseLawOutOfCredit` notices `thread`'s log holds.
fn notices(rig: &mut Rig, thread: ThreadId) -> usize {
    rig.log(thread)
        .iter()
        .filter(|event| {
            matches!(event, DomainEvent::Notice(notice)
                if notice.kind == Some(NoticeKind::CaseLawOutOfCredit))
        })
        .count()
}

/// Sends prompt `n` of `thread` and holds its answer back, so the turn stays live; returns the
/// turn's origin.
pub(super) fn live_turn(rig: &mut Rig, n: u128, thread: ThreadId) -> Origin {
    let turn = send_prompt(rig, n, thread);
    rig.hold_next("the prompt's answer", prompt_answer);
    Origin::Turn(turn)
}

/// Lets the held answer in and waits for the turn's end.
fn end_turn(rig: &mut Rig, thread: ThreadId) {
    rig.release_held();
    until_idle(rig, thread);
}

#[test]
fn a_refusal_marks_its_thread_once_and_no_other() {
    let mut rig = rig("credit-one-thread", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    let other = rig.thread_by(20);
    live_turn(&mut rig, 10, thread);
    rig.actor.on_input(refused(key(&rig, thread)));
    end_turn(&mut rig, thread);
    assert_eq!(notices(&mut rig, thread), 1);
    assert_eq!(notices(&mut rig, other), 0);
    rig.finish();
}

#[test]
fn two_refusals_in_a_turn_log_one_and_the_next_turn_logs_another() {
    let mut rig = rig("credit-per-turn", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    live_turn(&mut rig, 10, thread);
    let session = key(&rig, thread);
    rig.actor.on_input(refused(session));
    rig.actor.on_input(refused(session));
    end_turn(&mut rig, thread);
    assert_eq!(notices(&mut rig, thread), 1);
    // After the turn, before the next: still that turn's.
    rig.actor.on_input(refused(session));
    rig.settle_commits();
    assert_eq!(notices(&mut rig, thread), 1);

    live_turn(&mut rig, 11, thread);
    rig.actor.on_input(refused(key(&rig, thread)));
    end_turn(&mut rig, thread);
    assert_eq!(notices(&mut rig, thread), 2);
    rig.finish();
}

#[test]
fn a_helper_s_refusal_through_the_lead_s_session_marks_the_lead_s_thread() {
    let mut rig = rig("credit-helper", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    let origin = live_turn(&mut rig, 10, thread);
    // The lead's helper goes on in the background past the turn's end.
    let event = AcpEvent::HelperInBackground {
        session: AgentSessionId::new("sess-1").unwrap(),
        origin,
        helper: CompactString::from("toolu_01Launch"),
    };
    let input = Input::Agent {
        thread,
        input: AgentInput::Event(Box::new(event)),
    };
    rig.actor.on_input(input);
    end_turn(&mut rig, thread);
    let session = key(&rig, thread);

    // A stopped launch's session marks nothing.
    let stale = SessionKey {
        launch: session.launch + 1000,
        ..session
    };
    rig.actor.on_input(refused(stale));
    rig.settle_commits();
    assert_eq!(notices(&mut rig, thread), 0);

    // The helper's call, through the lead's relay session, marks the lead's thread.
    rig.actor.on_input(refused(session));
    rig.settle_commits();
    assert_eq!(notices(&mut rig, thread), 1);
    rig.finish();
}

#[test]
fn the_relay_s_other_events_log_no_notice() {
    let mut rig = rig("credit-other-events", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    live_turn(&mut rig, 10, thread);
    let session = key(&rig, thread);
    rig.actor
        .on_input(relay(RelayEvent::PaidResult { session }));
    rig.actor.on_input(relay(RelayEvent::CredentialEnded {
        session,
        ended: CredentialEnd::SignedOut,
    }));
    end_turn(&mut rig, thread);
    assert_eq!(notices(&mut rig, thread), 0);
    rig.finish();
}
