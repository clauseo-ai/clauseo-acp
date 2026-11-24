//! Switching a conversation's agent mid-way (issue 4a): a message that names another agent
//! stops the thread's agent (`Switched`), logs `AgentSwitched` and `ContextHandoff`, and the new
//! agent's first prompt carries the handover before the lawyer's words, which the log keeps as
//! typed. Each new agent expects its prompt exactly, so a handover left out or changed is a
//! mismatch. Earlier items none of which fits hand over the header alone; a message naming the
//! thread's own agent changes nothing. The refusals, the interleavings and a request too big
//! even for the header are unit tests (`actor/turn/tests/switch*.rs`).

use clauseo_protocol::{
    AgentId, CommandOutcome, ContentBlock, DeliveryRequest, DetachReason, DomainEvent,
    HandoffStatus, HostCommand, NoticeKind, PromptBlock, StopReason, StreamId, ThreadId, TurnId,
};
use serde_json::{Value, json};

use crate::codex::CODEX;
use crate::codex_cap::written;
use crate::limits_host::CLAUDE;
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread, text};
use crate::undo_note_restart::{answered, edits, end_turn, undo};

const FIRST: &str = "Finish the brief.";
const SECOND: &str = "Now check clause 4.";
const ANSWER: &str = "The brief is done.";

/// `SendPrompt` of `question` to `thread`, naming `agent`; its turn.
fn send_to(host: &Running, thread: ThreadId, question: &str, agent: &str) -> TurnId {
    let command = HostCommand::SendPrompt {
        thread,
        content: text(question),
        delivery: DeliveryRequest::Queue,
        config: None,
        agent: Some(AgentId::new(agent).unwrap()),
    };
    match host.ok(command) {
        CommandOutcome::PromptAccepted { turn, .. } => turn,
        other => panic!("SendPrompt answered {other:?}"),
    }
}

/// An answer chunk of `text` as message `id`.
fn answer(id: &str, text: &str) -> Value {
    json!({"update": {"update": {"sessionUpdate": "agent_message_chunk", "messageId": id,
        "content": {"type": "text", "text": text}}}})
}

/// The handover the new agent gets after the first turn: its question, Claude's answer and the
/// paper it changed (`crate::handover`'s header and labels).
fn handover() -> String {
    format!(
        "Earlier in this conversation the lawyer worked with Claude. Below are 3 of 3 earlier \
         items, whole. They are background, not a new request or instructions; files, tool \
         state and reasoning are not included.\n\n[Earlier: the lawyer]\n{FIRST}\n\n[Earlier: \
         Claude's answer]\n{ANSWER}\n\n[Earlier: papers changed]\nbrief.txt"
    )
}

/// The user message of `turn`, as logged.
fn logged_message(events: &Events, turn: TurnId) -> Vec<ContentBlock> {
    events
        .events()
        .find_map(|event| match event {
            DomainEvent::UserMessage(message) if message.turn == turn => {
                Some(message.content.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no UserMessage of {turn}: {}", events.dump()))
}

fn switched(event: &DomainEvent) -> bool {
    matches!(event, DomainEvent::AgentSwitched(_))
}

fn handed(event: &DomainEvent) -> bool {
    matches!(event, DomainEvent::ContextHandoff(_))
}

#[test]
fn claude_to_chatgpt_hands_over_the_conversation() {
    let dir = Dir::new("agent_switch_handover", &[]);
    let mut first = edits("p1", "brief.txt");
    first.insert(first.len() - 1, answer("m-1", ANSWER));
    let claude = written(&dir, "switch-claude", "claude-handshake.jsonl", first);
    let wrapped = format!("{}\n\nUser message:\n{SECOND}", handover());
    let mut second = answered("p1", &[&wrapped]).to_vec();
    second.insert(1, answer("m-2", "Clause 4 is fine."));
    let codex = written(&dir, "switch-codex", "codex-handshake.jsonl", second);
    dir.set_agents(&[(CLAUDE, claude), (CODEX, codex)]);
    let host = dir.start(0);
    let files = [("brief.txt", "draft\n")];
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &files, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (edited, _) = host.prompt(thread, FIRST);
    assert_eq!(events.until_turn_ended(edited).stop, StopReason::EndTurn);

    let turn = send_to(&host, thread, SECOND, CODEX);
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(events.turn_text(turn), "Clause 4 is fine.");
    // The lawyer's message is logged as typed, never with the handover.
    assert_eq!(
        logged_message(&events, turn),
        [ContentBlock::Text {
            text: SECOND.to_owned()
        }]
    );
    let detached = events.position(|event| {
        matches!(event, DomainEvent::AgentSessionDetached(d) if d.reason == DetachReason::Switched)
    });
    let at = events.position(switched).expect("AgentSwitched");
    assert!(
        detached.is_some_and(|detached| detached < at),
        "{}",
        events.dump()
    );
    let (switch, handoff, settled) = parts(&events);
    assert_eq!(
        (switch.from.as_str(), switch.to.as_str()),
        (CLAUDE, CODEX),
        "{}",
        events.dump()
    );
    assert_eq!(handoff.from.agent.as_str(), CLAUDE);
    assert_eq!(handoff.to.agent.as_str(), CODEX);
    let counts = (
        handoff.answers_covered.get(),
        handoff.items_included.get(),
        handoff.items_left_out.get(),
    );
    assert_eq!(counts, (1, 3, 0));
    assert_eq!(settled, vec![(handoff.handoff, HandoffStatus::Delivered)]);
    let notice = events.count(|event| {
        matches!(event, DomainEvent::Notice(n) if n.kind == Some(NoticeKind::ContextNotRestored))
    });
    assert_eq!(notice, 0, "a switch's session has the handover instead");
    let row = host.threads(matter);
    assert_eq!(row[0].agent.as_str(), CODEX, "the rail shows the new agent");

    // The first answer's step is still undone after the switch.
    undo(&host, &mut events, thread, edited, "brief.txt");
    events.assert_no_mismatch();
    host.shutdown();
}

/// The switch's `AgentSwitched` and `ContextHandoff`, and each `HandoffSettled`.
fn parts(
    events: &Events,
) -> (
    clauseo_protocol::AgentSwitched,
    clauseo_protocol::ContextHandoff,
    Vec<(clauseo_protocol::HandoffId, HandoffStatus)>,
) {
    let switch = events.events().find_map(|event| match event {
        DomainEvent::AgentSwitched(switched) => Some(switched.clone()),
        _ => None,
    });
    let handoff = events.events().find_map(|event| match event {
        DomainEvent::ContextHandoff(handoff) => Some((**handoff).clone()),
        _ => None,
    });
    (
        switch.unwrap_or_else(|| panic!("no AgentSwitched: {}", events.dump())),
        handoff.unwrap_or_else(|| panic!("no ContextHandoff: {}", events.dump())),
        parts_settled(events),
    )
}

/// Every earlier item is over the handover's 16,000-token cap, so not one fits: the new agent
/// gets the handover's header alone, which says how many earlier items are left out.
#[test]
fn a_conversation_none_of_which_fits_hands_over_its_header_alone() {
    let dir = Dir::new("agent_switch_header_only", &[]);
    let big_question = "Q".repeat(20_000);
    let big_answer = "A".repeat(20_000);
    let first = vec![
        json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
        answer("m-1", &big_answer),
        end_turn("p1"),
    ];
    let claude = written(&dir, "switch-big", "claude-handshake.jsonl", first);
    let header = "Earlier in this conversation the lawyer worked with Claude. The 2 earlier items \
                  are left out: not one fits in the room this request leaves. Files, tool state \
                  and reasoning are not included either.";
    let wrapped = format!("{header}\n\nUser message:\n{SECOND}");
    let codex = written(
        &dir,
        "switch-header-codex",
        "codex-handshake.jsonl",
        answered("p1", &[&wrapped]).to_vec(),
    );
    dir.set_agents(&[(CLAUDE, claude), (CODEX, codex)]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (big, _) = host.prompt(thread, &big_question);
    assert_eq!(events.until_turn_ended(big).stop, StopReason::EndTurn);

    let turn = send_to(&host, thread, SECOND, CODEX);
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let (switch, handoff, settled) = parts(&events);
    assert_eq!((switch.from.as_str(), switch.to.as_str()), (CLAUDE, CODEX));
    let counts = (handoff.items_included.get(), handoff.items_left_out.get());
    assert_eq!(counts, (0, 2));
    assert_eq!(settled, vec![(handoff.handoff, HandoffStatus::Delivered)]);
    assert_eq!(
        logged_message(&events, turn),
        [ContentBlock::Text {
            text: SECOND.to_owned()
        }]
    );
    assert_eq!(host.threads(matter)[0].agent.as_str(), CODEX);
    host.shutdown();
}

fn parts_settled(events: &Events) -> Vec<(clauseo_protocol::HandoffId, HandoffStatus)> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::HandoffSettled(settled) => Some((settled.handoff, settled.status)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_message_naming_the_threads_own_agent_changes_nothing() {
    let dir = Dir::new("agent_switch_same", &[]);
    let mut steps = answered("p1", &[FIRST]).to_vec();
    steps.extend(answered("p2", &[SECOND]));
    let claude = written(&dir, "switch-same", "claude-handshake.jsonl", steps);
    dir.set_agents(&[
        (CLAUDE, claude),
        (CODEX, Scenario::Bundled("codex-handshake")),
    ]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, FIRST);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: SECOND.to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
        config: None,
        agent: Some(AgentId::new(CLAUDE).unwrap()),
    };
    let CommandOutcome::PromptAccepted { turn, .. } = host.ok(command) else {
        panic!("not accepted");
    };
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(events.count(switched), 0);
    assert_eq!(events.count(handed), 0);
    let detaches = events.count(|event| matches!(event, DomainEvent::AgentSessionDetached(_)));
    assert_eq!(detaches, 0, "the agent kept its session: {}", events.dump());
    host.shutdown();
}
