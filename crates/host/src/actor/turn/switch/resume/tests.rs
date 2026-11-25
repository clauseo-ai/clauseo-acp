//! Reading a thread's log for a switch back: the session an agent last had in this thread, and
//! the last turn that session saw.

use clauseo_protocol::{
    AgentCapabilities, AgentId, AgentSessionAttached, AgentSessionId, AgentSwitched,
    ContextHandoff, Count, Delivery, DomainEvent, Endpoint, Envelope, EventId, HandoffId,
    HandoffSettled, HandoffStatus, MatterId, MessageId, Seq, StopReason, StreamId, TextAppended,
    TextKind, ThreadCreated, ThreadId, TurnEnded, TurnId, UserMessage,
};

use super::{Since, last_session, seen_until, since_switch};
use crate::pipeline::tests::support::{id, ts};

const CLAUDE: &str = "claude-acp";
const CODEX: &str = "codex-acp";

fn agent(name: &str) -> AgentId {
    AgentId::new(name).unwrap()
}

fn session(name: &str) -> AgentSessionId {
    AgentSessionId::new(name).unwrap()
}

fn turn(n: u64) -> TurnId {
    id(100 + n)
}

fn envelopes(events: Vec<DomainEvent>) -> Vec<Envelope> {
    events
        .into_iter()
        .zip(1_u64..)
        .map(|(event, n)| {
            let stream = StreamId::Thread(id::<ThreadId>(1));
            let seq = Seq::new(n).unwrap();
            Envelope::new(id::<EventId>(1_000 + n), stream, seq, ts(0), None, event)
        })
        .collect()
}

fn created(name: &str) -> DomainEvent {
    DomainEvent::ThreadCreated(ThreadCreated {
        matter: id::<MatterId>(2),
        agent: agent(name),
        title: None,
    })
}

fn attached(name: &str) -> DomainEvent {
    DomainEvent::AgentSessionAttached(AgentSessionAttached {
        session: session(name),
        agent_version: None,
        capabilities: AgentCapabilities::default(),
    })
}

fn switched(from: &str, to: &str) -> DomainEvent {
    DomainEvent::AgentSwitched(AgentSwitched {
        from: agent(from),
        to: agent(to),
    })
}

/// The lawyer's message of turn `n`, message id `200 + n`.
fn asked(n: u64) -> DomainEvent {
    DomainEvent::UserMessage(UserMessage {
        turn: turn(n),
        message: id::<MessageId>(200 + n),
        acp_message_id: None,
        content: Vec::new(),
        delivery: Delivery::Queue,
        config: std::collections::BTreeMap::new(),
        agent: None,
        copy_of: None,
    })
}

fn answer(n: u64) -> DomainEvent {
    DomainEvent::TextAppended(TextAppended {
        turn: turn(n),
        message: id::<MessageId>(300 + n),
        acp_message_id: None,
        kind: TextKind::Answer,
        text: "An answer.".to_owned(),
    })
}

fn ended(n: u64) -> DomainEvent {
    DomainEvent::TurnEnded(TurnEnded {
        turn: turn(n),
        stop: StopReason::EndTurn,
        usage: None,
        checkpoint_after: None,
        code_mode: false,
        cause: None,
    })
}

/// A handover `h` to `to` with the lawyer's message of turn `n`.
fn handed(h: u64, n: u64, to: &str) -> DomainEvent {
    let end = |name: &str| Endpoint {
        agent: agent(name),
        model: None,
        effort: None,
    };
    DomainEvent::ContextHandoff(Box::new(ContextHandoff {
        handoff: id::<HandoffId>(h),
        message: id::<MessageId>(200 + n),
        from: end(if to == CLAUDE { CODEX } else { CLAUDE }),
        to: end(to),
        answers_covered: Count::clamped(1),
        items_included: Count::clamped(1),
        items_left_out: Count::clamped(0),
        resumed: false,
    }))
}

fn settled(h: u64, status: HandoffStatus) -> DomainEvent {
    DomainEvent::HandoffSettled(HandoffSettled {
        handoff: id::<HandoffId>(h),
        status,
    })
}

/// Claude answers turn 1 in `sess-c`; the thread switches to Codex, which answers turn 2 in
/// `sess-x`.
fn claude_then_codex() -> Vec<DomainEvent> {
    vec![
        created(CLAUDE),
        attached("sess-c"),
        asked(1),
        answer(1),
        ended(1),
        switched(CLAUDE, CODEX),
        asked(2),
        attached("sess-x"),
        answer(2),
        ended(2),
    ]
}

#[test]
fn each_agents_last_session_is_its_own_in_this_log() {
    let mut events = claude_then_codex();
    // Claude came back once and could not load `sess-c`: its last session is the new one.
    events.extend([switched(CODEX, CLAUDE), attached("sess-c2")]);
    let log = envelopes(events);
    assert_eq!(last_session(&log, &agent(CLAUDE)), Some(session("sess-c2")));
    assert_eq!(last_session(&log, &agent(CODEX)), Some(session("sess-x")));
    assert_eq!(last_session(&log, &agent("gemini-acp")), None);
    // A session id the log never attached for an agent is never its own.
    let log = envelopes(claude_then_codex());
    assert_eq!(last_session(&log, &agent(CLAUDE)), Some(session("sess-c")));
}

#[test]
fn a_session_saw_up_to_its_agents_last_answer_there() {
    let log = envelopes(claude_then_codex());
    assert_eq!(
        seen_until(&log, &agent(CLAUDE), &session("sess-c")),
        Some(turn(1))
    );
    assert_eq!(
        seen_until(&log, &agent(CODEX), &session("sess-x")),
        Some(turn(2))
    );
    // Codex's answer in a session id Claude had would not count for Claude.
    assert_eq!(seen_until(&log, &agent(CLAUDE), &session("sess-x")), None);
}

#[test]
fn a_new_session_saw_nothing_its_agent_answered_in_an_older_one() {
    let mut events = claude_then_codex();
    // Back to Claude: `sess-c` would not load, so `sess-c2` started, and its turn failed before
    // anything reached it.
    events.extend([
        switched(CODEX, CLAUDE),
        asked(3),
        attached("sess-c2"),
        ended(3),
    ]);
    let log = envelopes(events);
    assert_eq!(seen_until(&log, &agent(CLAUDE), &session("sess-c2")), None);
    assert_eq!(
        seen_until(&log, &agent(CLAUDE), &session("sess-c")),
        Some(turn(1))
    );
}

#[test]
fn a_delivered_handover_counts_as_seen_and_a_failed_one_does_not() {
    for (status, seen) in [
        (HandoffStatus::Delivered, Some(turn(2))),
        (HandoffStatus::Failed, None),
    ] {
        let log = envelopes(vec![
            created(CLAUDE),
            attached("sess-c"),
            asked(1),
            answer(1),
            ended(1),
            switched(CLAUDE, CODEX),
            asked(2),
            handed(9, 2, CODEX),
            attached("sess-x"),
            settled(9, status),
            // Codex gave no answer: the turn was cancelled once the prompt went out.
            ended(2),
        ]);
        assert_eq!(
            seen_until(&log, &agent(CODEX), &session("sess-x")),
            seen,
            "{status:?}"
        );
    }
}

#[test]
fn the_later_of_an_answer_and_a_delivered_handover_counts() {
    let mut events = claude_then_codex();
    events.extend([
        switched(CODEX, CLAUDE),
        asked(3),
        handed(9, 3, CLAUDE),
        attached("sess-c"),
        settled(9, HandoffStatus::Delivered),
        ended(3),
        switched(CLAUDE, CODEX),
        asked(4),
        attached("sess-x"),
        answer(4),
        ended(4),
    ]);
    let log = envelopes(events);
    assert_eq!(
        seen_until(&log, &agent(CLAUDE), &session("sess-c")),
        Some(turn(3))
    );
    assert_eq!(
        seen_until(&log, &agent(CODEX), &session("sess-x")),
        Some(turn(4))
    );
}

#[test]
fn an_answer_in_a_turn_that_never_ended_counts_for_nothing() {
    let log = envelopes(vec![
        created(CLAUDE),
        attached("sess-c"),
        asked(1),
        answer(1),
        ended(1),
        asked(2),
        answer(2),
    ]);
    assert_eq!(
        seen_until(&log, &agent(CLAUDE), &session("sess-c")),
        Some(turn(1))
    );
}

/// Back to Claude after `claude_then_codex`, with `after` logged after the switch.
fn back_to_claude(after: Vec<DomainEvent>) -> Since {
    let mut events = claude_then_codex();
    events.extend([switched(CODEX, CLAUDE), asked(3)]);
    events.extend(after);
    since_switch(&envelopes(events), &agent(CLAUDE))
}

#[test]
fn a_handover_held_for_an_attach_that_never_came_is_owed() {
    let since = back_to_claude(vec![ended(3)]);
    assert_eq!(since.from, Some(agent(CODEX)));
    assert!(since.owes(), "{since:?}");
}

#[test]
fn a_logged_handover_not_delivered_is_owed_and_a_delivered_one_is_not() {
    let failed = back_to_claude(vec![
        handed(9, 3, CLAUDE),
        attached("sess-c"),
        settled(9, HandoffStatus::Failed),
        ended(3),
    ]);
    assert!(failed.owes(), "{failed:?}");
    let delivered = back_to_claude(vec![
        attached("sess-c"),
        handed(9, 3, CLAUDE),
        settled(9, HandoffStatus::Delivered),
        ended(3),
    ]);
    assert!(!delivered.owes(), "{delivered:?}");
}

#[test]
fn a_session_that_attached_with_nothing_to_take_is_owed_nothing() {
    let since = back_to_claude(vec![attached("sess-c"), ended(3)]);
    assert!(!since.owes(), "{since:?}");
}

#[test]
fn a_handover_too_long_for_another_agent_owes_the_threads_agent_nothing() {
    // On Codex, which had its handover; a switch to Claude did not fit and did not happen.
    let mut events = claude_then_codex();
    events.insert(7, handed(8, 2, CODEX));
    events.insert(9, settled(8, HandoffStatus::Delivered));
    events.extend([
        asked(3),
        handed(9, 3, CLAUDE),
        settled(9, HandoffStatus::Failed),
    ]);
    let since = since_switch(&envelopes(events), &agent(CODEX));
    assert_eq!(since.from, Some(agent(CLAUDE)));
    assert!(!since.owes(), "{since:?}");
    // On the agent the thread was created with and never switched to: owed nothing.
    let events = vec![
        created(CLAUDE),
        attached("sess-c"),
        asked(1),
        handed(9, 1, CODEX),
    ];
    let since = since_switch(&envelopes(events), &agent(CLAUDE));
    assert_eq!(since.from, None);
    assert!(!since.owes(), "{since:?}");
}
