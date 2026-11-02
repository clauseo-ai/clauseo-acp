//! The queue and steer in the view: a queued prompt shows `Queued` until its turn starts, an
//! edit replaces its content, a steer's outcome moves it to the turn that answers it, a tool call
//! a steer set aside is marked background with its finish time, whether a steer joins the turn
//! comes from the agent's kind and then its session, and `queue_held` is the host's flag.

use clauseo_protocol::{
    AgentCapabilities, AgentSessionAttached, ContentBlock, DomainEvent, HeaderEntry, MessageId,
    QueuedPromptEdited, SteerOutcome, SteerResolved, SteeringCapability, ThreadCreated, ThreadId,
    ToolCallStatus, TurnId, TurnQueued, TurnStarted, UserMessage, UserMessageEntry,
    UserMessageStatus, ViewEntry,
};
use clauseo_reducer::{Applied, Snapshot, StreamState, ViewChange, apply, set_queue_held};

use crate::fixtures::{
    Log, fixture_matter, start_turn, thread_created, tool_started, tool_status, turn_ended,
    user_message,
};
use crate::helpers::{fold, prompt_status, round_trip};

#[cfg(test)]
fn created() -> (Log, StreamState, ThreadId) {
    let mut log = Log::thread(31).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let state = fold(log.events());
    let thread = state.as_thread().unwrap().thread();
    (log, state, thread)
}

#[cfg(test)]
fn push(log: &mut Log, state: &mut StreamState, event: impl Into<DomainEvent>) -> Applied {
    let out = apply(state, &log.push(event).unwrap()).unwrap();
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    out
}

/// A prompt queued behind `ahead` of its own thread: its `UserMessage` and `TurnQueued`.
#[cfg(test)]
fn queue(
    log: &mut Log,
    state: &mut StreamState,
    thread: ThreadId,
    ahead: TurnId,
    text: &str,
) -> (TurnId, MessageId) {
    let (turn, message) = (log.fresh().unwrap(), log.fresh().unwrap());
    push(log, state, user_message(turn, message, text));
    let queued = TurnQueued::new(thread, turn, thread, Some(ahead)).unwrap();
    push(log, state, queued);
    (turn, message)
}

#[cfg(test)]
fn start(log: &mut Log, state: &mut StreamState, turn: TurnId) -> Applied {
    let checkpoint_before = log.fresh().unwrap();
    push(
        log,
        state,
        TurnStarted {
            turn,
            checkpoint_before,
        },
    )
}

#[cfg(test)]
fn prompt(state: &StreamState, message: MessageId) -> UserMessageEntry {
    state
        .entries()
        .values()
        .find_map(|entry| match entry {
            ViewEntry::UserMessage(prompt) if prompt.message == message => Some(prompt.clone()),
            _ => None,
        })
        .unwrap()
}

#[cfg(test)]
fn header(state: &StreamState) -> HeaderEntry {
    state.as_thread().unwrap().header().unwrap().clone()
}

#[test]
fn queued_prompt_waits_then_is_sent() {
    let (mut log, _, thread) = created();
    let first = start_turn(&mut log, "Read the lease.").unwrap();
    let mut state = fold(log.events());
    let (second, message) = queue(&mut log, &mut state, thread, first, "Then clause 3.");
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Queued)
    );
    let checkpoint = log.fresh().unwrap();
    push(&mut log, &mut state, turn_ended(first, checkpoint).unwrap());
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Queued),
        "it waits until its own turn starts"
    );
    // Through a snapshot, the queued prompt is still known as waiting.
    assert_eq!(round_trip(&state), state);
    let out = start(&mut log, &mut state, second);
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Sent)
    );
    assert!(out.changes.iter().any(|change| matches!(change,
        ViewChange::Upsert(entry) if matches!(&**entry, ViewEntry::UserMessage(p) if p.message == message))));
    assert_eq!(state, fold(log.events()));
}

#[test]
fn removed_queued_prompt_stays_in_the_view() {
    let (mut log, _, thread) = created();
    let first = start_turn(&mut log, "Read the lease.").unwrap();
    let mut state = fold(log.events());
    let (second, message) = queue(&mut log, &mut state, thread, first, "Then clause 3.");
    crate::helpers::cancel(&mut log, &mut state, second);
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Removed)
    );
    assert_eq!(state, fold(log.events()));
    assert_eq!(round_trip(&state), state);
}

#[test]
fn edit_replaces_a_queued_prompt() {
    let (mut log, _, thread) = created();
    let first = start_turn(&mut log, "Read the lease.").unwrap();
    let mut state = fold(log.events());
    let (second, message) = queue(&mut log, &mut state, thread, first, "Then clause 3.");
    let content = vec![
        ContentBlock::Text {
            text: "Then clause 4.".to_owned(),
        },
        ContentBlock::ResourceLink {
            uri: "file:///m/lease.docx".to_owned(),
            name: "lease.docx".to_owned(),
            mime_type: None,
            title: None,
            description: None,
            size: None,
        },
    ];
    let edited = QueuedPromptEdited {
        turn: second,
        message,
        content: content.clone(),
    };
    let out = push(&mut log, &mut state, edited.clone());
    assert_eq!(out.changes.len(), 1, "{:?}", out.changes);
    let shown = prompt(&state, message);
    assert_eq!(shown.content, content);
    assert!(shown.edited);
    assert_eq!(shown.status, UserMessageStatus::Queued);
    // The same edit again changes nothing.
    let out = push(&mut log, &mut state, edited);
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn injected_steer_marks_tools_aside_with_their_finish_time() {
    let (mut log, _, _) = created();
    let running = start_turn(&mut log, "Read the lease.").unwrap();
    let mut state = fold(log.events());
    let (search, done) = (log.fresh().unwrap(), log.fresh().unwrap());
    push(
        &mut log,
        &mut state,
        tool_started(running, search, 1).unwrap(),
    );
    push(
        &mut log,
        &mut state,
        tool_started(running, done, 2).unwrap(),
    );
    push(
        &mut log,
        &mut state,
        tool_status(done, ToolCallStatus::Completed),
    );
    let message = log.fresh().unwrap();
    push(
        &mut log,
        &mut state,
        UserMessage {
            turn: running,
            message,
            acp_message_id: None,
            content: vec![ContentBlock::Text {
                text: "Focus on clause 4".to_owned(),
            }],
            delivery: clauseo_protocol::Delivery::Steer,
        },
    );
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Sent)
    );
    let resolved = SteerResolved {
        message,
        outcome: SteerOutcome::Injected { turn: running },
        // A finished one listed by a race is not marked.
        aside: vec![search, done],
    };
    push(&mut log, &mut state, resolved);
    assert_eq!(
        prompt(&state, message).steer,
        Some(SteerOutcome::Injected { turn: running })
    );
    let tool = |state: &StreamState, id| {
        state
            .entries()
            .values()
            .find_map(|entry| match entry {
                ViewEntry::ToolCall(call) if call.tool_call == id => Some(call.clone()),
                _ => None,
            })
            .unwrap()
    };
    let mark = tool(&state, search).background.unwrap();
    assert_eq!((mark.steer, mark.finished_at), (message, None));
    assert_eq!(tool(&state, done).background, None);
    let finished = log
        .push(tool_status(search, ToolCallStatus::Completed))
        .unwrap();
    apply(&mut state, &finished).unwrap();
    let mark = tool(&state, search).background.unwrap();
    assert_eq!(mark.finished_at, Some(finished.ts));
    assert_eq!(state, fold(log.events()));
    assert_eq!(round_trip(&state), state);
}

#[test]
fn requeued_steer_waits_as_its_new_turn() {
    let (mut log, _, thread) = created();
    let steered = start_turn(&mut log, "Read the lease.").unwrap();
    let mut state = fold(log.events());
    let message = log.fresh().unwrap();
    push(
        &mut log,
        &mut state,
        UserMessage {
            turn: steered,
            message,
            acp_message_id: None,
            content: vec![ContentBlock::Text {
                text: "Focus on clause 4".to_owned(),
            }],
            delivery: clauseo_protocol::Delivery::Steer,
        },
    );
    let turn: TurnId = log.fresh().unwrap();
    let resolved = SteerResolved {
        message,
        outcome: SteerOutcome::Requeued { turn },
        aside: Vec::new(),
    };
    push(&mut log, &mut state, resolved);
    let queued = TurnQueued::new(thread, turn, thread, Some(steered)).unwrap();
    push(&mut log, &mut state, queued);
    let shown = prompt(&state, message);
    assert_eq!(
        (shown.turn, shown.status),
        (turn, UserMessageStatus::Queued)
    );
    assert_eq!(round_trip(&state), state);
    let checkpoint = log.fresh().unwrap();
    push(
        &mut log,
        &mut state,
        turn_ended(steered, checkpoint).unwrap(),
    );
    start(&mut log, &mut state, turn);
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Sent)
    );
    assert_eq!(state, fold(log.events()));
}

#[test]
fn steer_joins_turn_from_kind_then_session() {
    let kinds = [
        ("claude-acp", true),
        ("codex-acp", false),
        ("fake-agent", false),
    ];
    for (agent, joins) in kinds {
        let mut log = Log::thread(32).unwrap();
        let created = ThreadCreated {
            matter: fixture_matter().unwrap(),
            agent: agent.parse().unwrap(),
            title: None,
        };
        log.push(created).unwrap();
        let state = fold(log.events());
        assert_eq!(header(&state).steer_joins_turn, joins, "{agent}");
    }
    // The session's capabilities decide once it attaches: both flags, or not.
    let (mut log, mut state, _) = created();
    let attach = |native, idle_fallback_to_host| AgentSessionAttached {
        session: "sess-1".parse().unwrap(),
        agent_version: None,
        capabilities: AgentCapabilities {
            steering: Some(SteeringCapability {
                native,
                idle_fallback_to_host,
            }),
            ..AgentCapabilities::default()
        },
    };
    push(&mut log, &mut state, attach(true, false));
    assert!(!header(&state).steer_joins_turn, "codex-acp's steer");
    let out = push(&mut log, &mut state, attach(true, true));
    assert!(header(&state).steer_joins_turn);
    assert_eq!(out.changes.len(), 1);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn queue_held_is_the_hosts_flag() {
    let (_, mut state, _) = created();
    let thread_state = state.as_thread_mut().unwrap();
    let change = set_queue_held(thread_state, true).unwrap();
    assert!(matches!(&change,
        ViewChange::Upsert(entry) if matches!(&**entry, ViewEntry::Header(h) if h.queue_held)));
    assert!(set_queue_held(thread_state, true).is_none(), "no change");
    assert!(header(&state).queue_held);
    // A snapshot writes it false: the host sets it again after a load.
    let bytes = Snapshot::of(&state).to_bytes().unwrap();
    let read = Snapshot::from_bytes(&bytes).unwrap().state;
    assert!(!header(&read).queue_held);
    assert!(header(&state).queue_held);
}
