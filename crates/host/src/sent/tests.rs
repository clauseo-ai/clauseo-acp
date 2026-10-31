//! Tests of the sent view's two layers, without a store.

use clauseo_protocol::{
    AgentCapabilities, AgentSessionAttached, AgentSessionDetached, AgentSessionId, Deliverable,
    DetachReason, DomainEvent, ElicitationExpired, ElicitationId, ElicitationRequest,
    ElicitationRequested, Envelope, EventId, ExpiryReason, MessageId, PermissionChoice,
    PermissionKind, PermissionOption, PermissionRequestId, PermissionRequested, PermissionResolved,
    Principal, Seq, StreamId, TextAppended, TextKind, ThreadId, ToolCallId, ToolCallPatch,
    ToolCallStarted, ToolCallStatus, ToolCallUpdated, ToolKind, TurnId,
};

use super::SentView;
use crate::pipeline::tests::support::{id, thread_stream, ts};

/// Envelopes of `stream` from `first`, one per event.
fn envelopes(stream: StreamId, first: u64, events: Vec<DomainEvent>) -> Vec<Envelope> {
    events
        .into_iter()
        .zip(first..)
        .map(|(event, seq)| {
            let event_id = id::<EventId>(1_000 + seq);
            Envelope::new(event_id, stream, Seq::new(seq).unwrap(), ts(0), None, event)
        })
        .collect()
}

fn session(name: &str) -> AgentSessionId {
    AgentSessionId::new(name).unwrap()
}

fn attached(name: &str) -> DomainEvent {
    DomainEvent::AgentSessionAttached(AgentSessionAttached {
        session: session(name),
        agent_version: None,
        capabilities: AgentCapabilities::default(),
    })
}

fn started(n: u64, acp_id: &str) -> DomainEvent {
    DomainEvent::ToolCallStarted(Box::new(ToolCallStarted {
        turn: id::<TurnId>(1),
        tool_call: id::<ToolCallId>(n),
        acp_id: acp_id.into(),
        title: "Read".into(),
        kind: ToolKind::Read,
        tool_name: None,
        status: ToolCallStatus::Pending,
        locations: Vec::new(),
        raw_input: None,
        raw_output: None,
        content: Vec::new(),
    }))
}

fn text(n: u64, acp_message_id: &str) -> DomainEvent {
    DomainEvent::TextAppended(TextAppended {
        turn: id::<TurnId>(1),
        message: id::<MessageId>(n),
        acp_message_id: Some(acp_message_id.into()),
        kind: TextKind::Answer,
        text: "hello".to_owned(),
    })
}

fn option() -> PermissionOption {
    PermissionOption {
        option_id: "allow".into(),
        kind: PermissionKind::AllowOnce,
        agent_label: "Allow".to_owned(),
    }
}

#[test]
fn replay_keys_are_per_session() {
    let stream = thread_stream();
    let log = envelopes(
        stream,
        1,
        vec![
            attached("s1"),
            started(10, "call-1"),
            text(20, "m-1"),
            text(20, "m-1"),
            DomainEvent::AgentSessionDetached(AgentSessionDetached {
                reason: DetachReason::Closed,
            }),
            attached("s2"),
            started(11, "call-1"),
        ],
    );
    let view = SentView::rebuild(stream, &log);
    assert_eq!(view.session(), Some(&session("s2")));
    let (first, _) = view.tool_call(&session("s1"), "call-1").unwrap();
    let (second, _) = view.tool_call(&session("s2"), "call-1").unwrap();
    assert_eq!((first, second), (id(10), id(11)));
    assert_eq!(view.message(&session("s1"), "m-1"), Some(id(20)));
    assert_eq!(view.message(&session("s2"), "m-1"), None);
    assert_eq!(view.unfinished_tools(id(1)).count(), 2);
    assert!(view.settled());
}

#[test]
fn sent_layer_leads_and_rolls_back() {
    let stream = thread_stream();
    let mut view = SentView::new(stream);
    let first = envelopes(stream, 1, vec![attached("s1"), started(10, "call-1")]);
    view.on_sent(&first);
    view.on_committed(&first);
    assert!(view.settled());

    let request = id::<PermissionRequestId>(30);
    let elicitation = id::<ElicitationId>(40);
    let in_flight = envelopes(
        stream,
        3,
        vec![
            DomainEvent::ToolCallUpdated(Box::new(ToolCallUpdated {
                tool_call: id(10),
                patch: ToolCallPatch {
                    status: Some(ToolCallStatus::Completed),
                    ..ToolCallPatch::default()
                },
            })),
            DomainEvent::PermissionRequested(PermissionRequested {
                request,
                tool_call: Some(id(10)),
                options: vec![option()],
                deliverable: Deliverable::Live,
            }),
            DomainEvent::ElicitationRequested(ElicitationRequested {
                elicitation,
                request: ElicitationRequest::Url {
                    message: "Sign in".to_owned(),
                    url: "https://example.com".to_owned(),
                },
            }),
        ],
    );
    // Another stream's envelopes in the same commit are skipped.
    let other = envelopes(
        StreamId::Thread(id::<ThreadId>(9)),
        1,
        vec![started(12, "x")],
    );
    view.on_sent(&in_flight);
    view.on_sent(&other);
    assert!(view.tool(id(10)).unwrap().finished);
    assert!(view.tool(id(12)).is_none());
    assert_eq!(view.permission(request).unwrap().options, [option()]);
    assert!(view.elicitation(elicitation).is_some());
    assert!(!view.settled());

    view.roll_back();
    assert!(view.settled());
    assert!(!view.tool(id(10)).unwrap().finished);
    assert!(view.permission(request).is_none());
    assert_eq!(view.permissions().count(), 0);
    assert_eq!(view.elicitations().count(), 0);

    // Answers remove what they settle.
    let settle = envelopes(
        stream,
        3,
        vec![
            DomainEvent::PermissionRequested(PermissionRequested {
                request,
                tool_call: None,
                options: vec![option()],
                deliverable: Deliverable::Live,
            }),
            DomainEvent::PermissionResolved(PermissionResolved {
                request,
                choice: PermissionChoice::Cancelled,
                by: Principal::LocalDesktop,
            }),
            DomainEvent::ElicitationExpired(ElicitationExpired {
                elicitation,
                reason: ExpiryReason::TurnEnded,
            }),
        ],
    );
    view.on_sent(&settle);
    assert!(view.permission(request).is_none());
}
