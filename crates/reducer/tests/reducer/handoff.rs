//! The agent switch: `AgentSwitched` resets the header for the new agent, a `ContextHandoff`
//! makes a pending handoff entry, its `HandoffSettled` settles it, and a snapshot keeps it.

use clauseo_protocol::{
    AgentNamed, AgentSwitched, AvailableCommand, CommandsAvailable, ConfigCategory, ConfigOption,
    ConfigOptionKind, ConfigOptionsReplaced, ContextHandoff, Count, DomainEvent, Endpoint, EntryId,
    HandoffEntry, HandoffEntryStatus, HandoffId, HandoffSettled, HandoffStatus, HeaderEntry,
    MessageId, ModeChanged, NamedChoice, Seq, Timestamp, TurnId, TurnStarted, ViewEntry,
};
use clauseo_reducer::{Anomaly, Applied, StreamState, ViewChange, apply};

use crate::fixtures::{Log, short, start_turn, thread_created, usage, user_message};
use crate::helpers::{fold, round_trip};

/// A handover of `handoff` from claude-acp (Opus, High) to codex-acp, with `message`.
#[cfg(test)]
pub(crate) fn handoff_event(handoff: HandoffId, message: MessageId) -> DomainEvent {
    ContextHandoff {
        handoff,
        message,
        from: Endpoint {
            agent: "claude-acp".parse().unwrap(),
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
            agent: "codex-acp".parse().unwrap(),
            model: None,
            effort: None,
        },
        answers_covered: Count::new(4).unwrap(),
        items_included: Count::new(9).unwrap(),
        items_left_out: Count::new(2).unwrap(),
        resumed: false,
    }
    .into()
}

#[cfg(test)]
fn created(n: u64) -> (Log, StreamState) {
    let mut log = Log::thread(n).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let state = fold(log.events());
    (log, state)
}

#[cfg(test)]
fn push(log: &mut Log, state: &mut StreamState, event: impl Into<DomainEvent>) -> Applied {
    apply(state, &log.push(event).unwrap()).unwrap()
}

#[cfg(test)]
fn header(state: &StreamState) -> &HeaderEntry {
    state.as_thread().unwrap().header().unwrap()
}

#[cfg(test)]
fn upserts(out: &Applied) -> Vec<&ViewEntry> {
    out.changes
        .iter()
        .filter_map(|change| match change {
            ViewChange::Upsert(entry) => Some(&**entry),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
fn handoff_entry(state: &StreamState, handoff: HandoffId) -> &HandoffEntry {
    state
        .entries()
        .values()
        .find_map(|entry| match entry {
            ViewEntry::Handoff(entry) if entry.handoff == handoff => Some(entry),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_switch_moves_the_header_to_the_new_agent_and_clears_what_the_old_session_reported() {
    let (mut log, mut state) = created(81);
    let filled: Vec<DomainEvent> = vec![
        AgentNamed {
            agent_name: short("Claude Code"),
        }
        .into(),
        ModeChanged {
            mode_id: short("plan"),
        }
        .into(),
        ConfigOptionsReplaced {
            options: vec![ConfigOption {
                id: short("fast"),
                name: short("Fast"),
                description: None,
                category: ConfigCategory::Other,
                kind: ConfigOptionKind::Boolean {
                    current_value: true,
                },
            }],
        }
        .into(),
        CommandsAvailable {
            commands: vec![AvailableCommand {
                name: short("review"),
                description: "Review the draft".to_owned(),
                input_hint: None,
            }],
        }
        .into(),
        usage(1000).unwrap(),
    ];
    for event in filled {
        let out = push(&mut log, &mut state, event);
        assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    }
    let before = header(&state).clone();
    assert!(before.steer_joins_turn, "claude-acp's steer joins by kind");
    assert!(before.agent_name.is_some() && before.mode_id.is_some() && before.usage.is_some());
    assert!(!before.config_options.is_empty() && !before.commands.is_empty());

    let switched = AgentSwitched {
        from: "claude-acp".parse().unwrap(),
        to: "codex-acp".parse().unwrap(),
    };
    let out = push(&mut log, &mut state, switched.clone());
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    let after = header(&state);
    assert_eq!(after.agent.as_str(), "codex-acp");
    assert_eq!(after.agent_name, None);
    assert_eq!(after.mode_id, None);
    assert!(
        after.config_options.is_empty(),
        "{:?}",
        after.config_options
    );
    assert!(after.commands.is_empty(), "{:?}", after.commands);
    assert_eq!(after.usage, None);
    assert!(after.tools.is_empty(), "{:?}", after.tools);
    assert!(!after.steer_joins_turn, "codex-acp's steer does not join");
    // The rest of the header stays.
    assert_eq!(
        (after.title.clone(), after.status),
        (before.title, before.status)
    );
    // One header upsert, holding the new header.
    assert_eq!(upserts(&out), [&ViewEntry::Header(Box::new(after.clone()))]);
    // A second switch to the same agent changes nothing.
    let again = push(&mut log, &mut state, switched);
    assert!(again.changes.is_empty(), "{:?}", again.changes);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn a_switch_clears_the_model_and_effort_the_old_agents_turn_ran_with() {
    let (mut log, _) = created(87);
    let turn: TurnId = log.fresh().unwrap();
    let message: MessageId = log.fresh().unwrap();
    log.push(user_message(turn, message, "Review it.")).unwrap();
    let checkpoint_before = log.fresh().unwrap();
    log.push(TurnStarted {
        turn,
        checkpoint_before,
        model: Some(NamedChoice {
            id: "opus".into(),
            name: "Opus".into(),
        }),
        effort: Some(NamedChoice {
            id: "high".into(),
            name: "High".into(),
        }),
    })
    .unwrap();
    let mut state = fold(log.events());
    assert!(header(&state).model.is_some() && header(&state).effort.is_some());
    let switched = AgentSwitched {
        from: "claude-acp".parse().unwrap(),
        to: "codex-acp".parse().unwrap(),
    };
    let out = push(&mut log, &mut state, switched);
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    let after = header(&state);
    assert_eq!((after.model.clone(), after.effort.clone()), (None, None));
    assert_eq!(upserts(&out), [&ViewEntry::Header(Box::new(after.clone()))]);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn a_handoff_makes_a_pending_entry_at_its_seq_that_settles_delivered() {
    let (mut log, _) = created(82);
    start_turn(&mut log, "Now in Codex.").unwrap();
    let mut state = fold(log.events());
    let handoff: HandoffId = log.fresh().unwrap();
    let message: MessageId = log.fresh().unwrap();
    let order = log.next_seq();
    let out = push(&mut log, &mut state, handoff_event(handoff, message));
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    let want = HandoffEntry {
        order: Seq::new(order).unwrap(),
        handoff,
        message,
        from: Endpoint {
            agent: "claude-acp".parse().unwrap(),
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
            agent: "codex-acp".parse().unwrap(),
            model: None,
            effort: None,
        },
        answers_covered: Count::new(4).unwrap(),
        items_included: Count::new(9).unwrap(),
        items_left_out: Count::new(2).unwrap(),
        resumed: false,
        status: HandoffEntryStatus::Pending,
        ts: Timestamp::new(i64::try_from(order * 1000).unwrap()).unwrap(),
    };
    assert_eq!(upserts(&out), [&ViewEntry::Handoff(want.clone())]);
    assert_eq!(handoff_entry(&state, handoff), &want);
    assert_eq!(
        ViewEntry::Handoff(want.clone()).id(),
        EntryId::Handoff(handoff)
    );

    let delivered = HandoffSettled {
        handoff,
        status: HandoffStatus::Delivered,
    };
    let out = push(&mut log, &mut state, delivered);
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    let settled = HandoffEntry {
        status: HandoffEntryStatus::Delivered,
        ..want
    };
    assert_eq!(upserts(&out), [&ViewEntry::Handoff(settled.clone())]);
    assert_eq!(handoff_entry(&state, handoff), &settled);
    // The same status again changes nothing.
    let again = push(&mut log, &mut state, delivered);
    assert!(again.changes.is_empty(), "{:?}", again.changes);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn a_resumed_handoff_makes_a_resumed_entry() {
    let (mut log, mut state) = created(86);
    let handoff: HandoffId = log.fresh().unwrap();
    let message: MessageId = log.fresh().unwrap();
    let DomainEvent::ContextHandoff(mut event) = handoff_event(handoff, message) else {
        unreachable!("handoff_event makes a ContextHandoff");
    };
    event.resumed = true;
    let out = push(&mut log, &mut state, DomainEvent::ContextHandoff(event));
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert!(handoff_entry(&state, handoff).resumed);
    let read = round_trip(&state);
    assert!(handoff_entry(&read, handoff).resumed, "a snapshot keeps it");
    assert_eq!(state, fold(log.events()));
    // One not resumed leaves the entry's field false.
    let other: HandoffId = log.fresh().unwrap();
    push(&mut log, &mut state, handoff_event(other, message));
    assert!(!handoff_entry(&state, other).resumed);
}

#[test]
fn a_failed_or_newer_status_settles_failed() {
    for status in [HandoffStatus::Failed, HandoffStatus::Unknown] {
        let (mut log, mut state) = created(83);
        let handoff: HandoffId = log.fresh().unwrap();
        let message = log.fresh().unwrap();
        push(&mut log, &mut state, handoff_event(handoff, message));
        let out = push(&mut log, &mut state, HandoffSettled { handoff, status });
        assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
        assert_eq!(upserts(&out).len(), 1);
        assert_eq!(
            handoff_entry(&state, handoff).status,
            HandoffEntryStatus::Failed
        );
    }
}

#[test]
fn an_unknown_or_repeated_handoff_is_an_anomaly() {
    let (mut log, mut state) = created(84);
    let stranger: HandoffId = log.fresh().unwrap();
    let settled = HandoffSettled {
        handoff: stranger,
        status: HandoffStatus::Delivered,
    };
    let out = push(&mut log, &mut state, settled);
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    assert_eq!(
        out.anomalies,
        [Anomaly::UnknownReference {
            event: "handoff_settled",
            target: EntryId::Handoff(stranger),
        }]
    );

    let handoff: HandoffId = log.fresh().unwrap();
    let message = log.fresh().unwrap();
    push(&mut log, &mut state, handoff_event(handoff, message));
    let out = push(&mut log, &mut state, handoff_event(handoff, message));
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    assert_eq!(
        out.anomalies,
        [Anomaly::Duplicate {
            event: "context_handoff",
            target: EntryId::Handoff(handoff),
        }]
    );
    let entries = state.entries().values();
    let handoffs = entries.filter(|entry| matches!(entry, ViewEntry::Handoff(_)));
    assert_eq!(handoffs.count(), 1);
}

#[test]
fn a_snapshot_keeps_the_handoff_and_settles_it_after_reading() {
    let (mut log, mut state) = created(85);
    let handoff: HandoffId = log.fresh().unwrap();
    let message = log.fresh().unwrap();
    push(&mut log, &mut state, handoff_event(handoff, message));
    let mut read = round_trip(&state);
    assert_eq!(read, state);
    // The rebuilt index finds the entry, so its settle applies to the read state.
    let settled = HandoffSettled {
        handoff,
        status: HandoffStatus::Failed,
    };
    let envelope = log.push(settled).unwrap();
    let out = apply(&mut read, &envelope).unwrap();
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert_eq!(
        handoff_entry(&read, handoff).status,
        HandoffEntryStatus::Failed
    );
    assert_eq!(read, fold(log.events()));
}
