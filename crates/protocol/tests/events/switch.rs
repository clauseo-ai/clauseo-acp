//! The agent switch: `AgentSwitched`, `ContextHandoff` and `HandoffSettled` with its open
//! enum `HandoffStatus`, `DetachReason::Switched`, the view's `HandoffEntryStatus`,
//! `SendPrompt`'s and `UserMessage`'s `agent`, and `NamedChoice` in `Endpoint`: wire forms,
//! round trips in JSON and `MessagePack`, and values from a newer Clauseo.

use clauseo_protocol::{
    AgentSessionDetached, AgentSwitched, CommandEnvelope, ContextHandoff, Delivery,
    DeliveryRequest, DetachReason, DomainEvent, Endpoint, HandoffEntryStatus, HandoffSettled,
    HandoffStatus, HostCommand, NamedChoice, PromptBlock, UserMessage,
};
use serde_json::json;

use crate::fixtures::{agent, count, handoff_id, message_id, thread_id, turn_id};

fn round_trips(event: &DomainEvent) -> serde_json::Value {
    let json = serde_json::to_value(event).unwrap();
    assert_eq!(
        &serde_json::from_value::<DomainEvent>(json.clone()).unwrap(),
        event
    );
    let packed = rmp_serde::to_vec_named(event).unwrap();
    assert_eq!(
        &rmp_serde::from_slice::<DomainEvent>(&packed).unwrap(),
        event
    );
    json
}

#[test]
fn a_switch_writes_both_agents() {
    let switched: DomainEvent = AgentSwitched {
        from: agent("claude-acp"),
        to: agent("codex-acp"),
    }
    .into();
    assert_eq!(switched.kind(), "agent_switched");
    assert_eq!(
        round_trips(&switched),
        json!({"agent_switched": {"from": "claude-acp", "to": "codex-acp"}})
    );
    let missing = json!({"agent_switched": {"from": "claude-acp"}});
    assert!(serde_json::from_value::<DomainEvent>(missing).is_err());
}

#[test]
fn a_handoff_writes_its_counts_and_leaves_out_an_unknown_model_and_effort() {
    let handoff: DomainEvent = ContextHandoff {
        handoff: handoff_id(19),
        message: message_id(5),
        from: Endpoint {
            agent: agent("claude-acp"),
            model: Some(NamedChoice {
                id: "opus".into(),
                name: "Opus".into(),
            }),
            effort: Some(NamedChoice {
                id: "high".into(),
                name: "High".into(),
            }),
        },
        to: Endpoint {
            agent: agent("codex-acp"),
            model: None,
            effort: None,
        },
        answers_covered: count(4),
        items_included: count(9),
        items_left_out: count(2),
        resumed: false,
    }
    .into();
    assert_eq!(handoff.kind(), "context_handoff");
    let handoff_text = serde_json::to_value(handoff_id(19)).unwrap();
    let message_text = serde_json::to_value(message_id(5)).unwrap();
    assert_eq!(
        round_trips(&handoff),
        json!({"context_handoff": {
            "handoff": handoff_text,
            "message": message_text,
            "from": {
                "agent": "claude-acp",
                "model": {"id": "opus", "name": "Opus"},
                "effort": {"id": "high", "name": "High"},
            },
            "to": {"agent": "codex-acp"},
            "answers_covered": 4,
            "items_included": 9,
            "items_left_out": 2,
        }})
    );
}

#[test]
fn a_resumed_handoff_says_so_and_a_full_one_leaves_the_field_out() {
    for resumed in [true, false] {
        let event: DomainEvent = ContextHandoff {
            handoff: handoff_id(19),
            message: message_id(5),
            from: Endpoint {
                agent: agent("codex-acp"),
                model: None,
                effort: None,
            },
            to: Endpoint {
                agent: agent("claude-acp"),
                model: None,
                effort: None,
            },
            answers_covered: count(1),
            items_included: count(2),
            items_left_out: count(0),
            resumed,
        }
        .into();
        let json = round_trips(&event);
        let body = &json["context_handoff"];
        if resumed {
            assert_eq!(body["resumed"], json!(true));
        } else {
            assert!(
                body.get("resumed").is_none(),
                "false is a missing field, so the shape before it stays: {json}"
            );
        }
    }
    // A handoff written before the field reads as not resumed.
    let handoff_text = serde_json::to_value(handoff_id(19)).unwrap();
    let message_text = serde_json::to_value(message_id(5)).unwrap();
    let old: DomainEvent = serde_json::from_value(json!({"context_handoff": {
        "handoff": handoff_text,
        "message": message_text,
        "from": {"agent": "claude-acp"},
        "to": {"agent": "codex-acp"},
        "answers_covered": 1,
        "items_included": 2,
        "items_left_out": 0,
    }}))
    .unwrap();
    let DomainEvent::ContextHandoff(old) = &old else {
        panic!("not context_handoff: {old:?}");
    };
    assert!(!old.resumed);
}

#[test]
fn a_settled_handoff_writes_its_status_and_a_newer_one_reads_as_unknown() {
    let handoff_text = serde_json::to_value(handoff_id(19)).unwrap();
    for (status, word) in [
        (HandoffStatus::Delivered, "delivered"),
        (HandoffStatus::Failed, "failed"),
    ] {
        let settled: DomainEvent = HandoffSettled {
            handoff: handoff_id(19),
            status,
        }
        .into();
        assert_eq!(settled.kind(), "handoff_settled");
        assert_eq!(
            round_trips(&settled),
            json!({"handoff_settled": {"handoff": handoff_text, "status": word}})
        );
    }
    let read: DomainEvent = serde_json::from_value(
        json!({"handoff_settled": {"handoff": handoff_text, "status": "rerouted"}}),
    )
    .unwrap();
    let DomainEvent::HandoffSettled(settled) = &read else {
        panic!("not handoff_settled: {read:?}");
    };
    assert_eq!(settled.status, HandoffStatus::Unknown);
    assert_eq!(
        HandoffStatus::ALL,
        [
            HandoffStatus::Delivered,
            HandoffStatus::Failed,
            HandoffStatus::Unknown
        ]
    );
}

#[test]
fn the_view_status_follows_the_logged_one_and_an_unknown_one_reads_as_failed() {
    let cases = [
        (HandoffStatus::Delivered, HandoffEntryStatus::Delivered),
        (HandoffStatus::Failed, HandoffEntryStatus::Failed),
        (HandoffStatus::Unknown, HandoffEntryStatus::Failed),
    ];
    for (logged, shown) in cases {
        assert_eq!(HandoffEntryStatus::from(logged), shown);
    }
    let words: Vec<String> = [
        HandoffEntryStatus::Pending,
        HandoffEntryStatus::Delivered,
        HandoffEntryStatus::Failed,
    ]
    .iter()
    .map(|status| serde_json::to_value(status).unwrap().to_string())
    .collect();
    assert_eq!(words, [r#""pending""#, r#""delivered""#, r#""failed""#]);
}

#[test]
fn a_switched_detach_is_a_plain_string() {
    let detached: DomainEvent = AgentSessionDetached {
        reason: DetachReason::Switched,
    }
    .into();
    assert_eq!(
        round_trips(&detached),
        json!({"agent_session_detached": {"reason": "switched"}})
    );
}

#[test]
fn send_prompt_names_its_agent_only_when_it_has_one() {
    let thread = serde_json::to_value(thread_id(2)).unwrap();
    let id = "01900000-0000-7000-8000-000000000009";
    let cases = [
        (
            json!({"send_prompt": {
                "thread": thread,
                "content": [{"text": {"text": "hi"}}],
                "agent": "codex-acp",
            }}),
            Some(agent("codex-acp")),
        ),
        (
            json!({"send_prompt": {"thread": thread, "content": [{"text": {"text": "hi"}}]}}),
            None,
        ),
    ];
    for (wire, named) in cases {
        let command = HostCommand::SendPrompt {
            thread: thread_id(2),
            content: vec![PromptBlock::Text {
                text: "hi".to_owned(),
            }],
            delivery: DeliveryRequest::Queue,
            config: None,
            agent: named,
        };
        let envelope: CommandEnvelope =
            serde_json::from_value(json!({"command_id": id, "command": wire.clone()})).unwrap();
        assert_eq!(envelope.command, command);
        assert_eq!(serde_json::to_value(&command).unwrap(), wire);
        assert!(envelope.validate().is_ok());
    }
}

#[test]
fn a_user_message_names_its_agent_only_when_it_has_one() {
    let turn = serde_json::to_value(turn_id(3)).unwrap();
    let message = serde_json::to_value(message_id(5)).unwrap();
    for named in [Some(agent("codex-acp")), None] {
        let event: DomainEvent = UserMessage {
            turn: turn_id(3),
            message: message_id(5),
            acp_message_id: None,
            content: Vec::new(),
            delivery: Delivery::Queue,
            config: std::collections::BTreeMap::new(),
            agent: named.clone(),
        }
        .into();
        let mut wire = json!({"user_message": {"turn": turn, "message": message, "content": []}});
        if named.is_some() {
            wire["user_message"]["agent"] = json!("codex-acp");
        }
        assert_eq!(
            round_trips(&event),
            wire,
            "missing when none, so older entries read"
        );
    }
}
