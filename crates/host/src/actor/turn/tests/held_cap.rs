//! The cap on agent output held for a turn (`own_turn/cap.rs`): while a turn of the thread is on
//! its way, so no agent-started turn opens, a background helper's words pile up; past the cap
//! the oldest words go, and everything else stays: a card the agent waits on, the step a card
//! points at, a helper's launch and its handback, and the costed usage that ends the agent's own
//! turn.

use clauseo_acp::{AcpEvent, AgentUpdate, MessageRole, Origin, PendingKey, ToolCall};
use clauseo_protocol::{
    AgentSessionId, ContentBlock, Cost, Count, DomainEvent, Finite, HelperOutcome, HostCommand,
    MatterPath, PermissionKind, PermissionOption, TextOrBlob, ThreadId, ToolCallStatus, ToolKind,
    ToolLocation,
};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::helpers::{
    COMPLETED, DELIVERED, call, feed, in_background, lead, live_turn, one_turn, patch,
};
use super::rig::Rig;
use crate::actor::turn::{TurnPhase, own_turn};

/// A helper's words, as they wait with no turn live.
fn words(text: String) -> AcpEvent {
    AcpEvent::HelperUpdate {
        session: AgentSessionId::new("sess-1").unwrap(),
        origin: Origin::Idle,
        helper: "toolu_A".into(),
        update: AgentUpdate::MessageChunk {
            role: MessageRole::Answer,
            acp_message_id: None,
            content: ContentBlock::Text { text },
        },
    }
}

/// The text of held words, its number for a chunk this test made.
fn number(event: &AcpEvent) -> Option<usize> {
    let AcpEvent::HelperUpdate {
        update:
            AgentUpdate::MessageChunk {
                content: ContentBlock::Text { text },
                ..
            },
        ..
    } = event
    else {
        return None;
    };
    text.split(':').next()?.parse().ok()
}

/// Twelve chunks of half a MiB each (three halves of the cap) wait behind a card: the oldest
/// chunks go, the newest stay in order and within the cap, and the card stays first.
#[test]
fn held_output_past_its_cap_drops_its_oldest_updates_and_keeps_its_cards() {
    let mut rig = Rig::start("held-cap", "");
    let thread = rig.thread();
    rig.current_turn(thread, TurnPhase::Starting);
    let card = AcpEvent::ElicitationCompleted {
        elicitation_id: "el-1".into(),
    };
    own_turn::hold_helper(&mut rig.actor, thread, card.clone());
    let half = "x".repeat(own_turn::HELD_CAP_BYTES / 8);
    for n in 0..12 {
        own_turn::hold_helper(&mut rig.actor, thread, words(format!("{n}:{half}")));
    }
    let held = own_turn::held_events(&mut rig.actor, thread);
    assert_eq!(held.first(), Some(&card));
    let kept: Vec<usize> = held.iter().filter_map(number).collect();
    assert_eq!(kept.len(), held.len() - 1, "every other event is a chunk");
    assert!(kept.len() < 12, "nothing was dropped: {kept:?}");
    let newest: Vec<usize> = (12 - kept.len()..12).collect();
    assert_eq!(kept, newest, "the newest stay, in order");
    assert!(kept.len() * half.len() <= own_turn::HELD_CAP_BYTES);
    // The turn was put in by hand; nothing would end it.
    rig.actor.threads.get_mut(&thread).unwrap().turn.current = None;
    rig.finish();
}

/// An eighth of the cap of words from a helper the host never started: they weigh in the pile
/// and are dropped unlogged once a turn holds them.
fn filler(n: usize) -> AcpEvent {
    let mut event = words(format!("{n}:{}", "x".repeat(own_turn::HELD_CAP_BYTES / 8)));
    if let AcpEvent::HelperUpdate { helper, .. } = &mut event {
        *helper = "toolu_never".into();
    }
    event
}

/// `update` of the lead's, with no prompt in flight.
fn idle(update: AgentUpdate) -> AcpEvent {
    AcpEvent::Update {
        session: AgentSessionId::new("sess-1").unwrap(),
        origin: Origin::Idle,
        update,
    }
}

/// The first prompt's turn starts the background helper `toolu_A` and ends by itself, leaving
/// it open.
fn first_turn_with_a_helper(rig: &mut Rig, thread: ThreadId) {
    let turn = live_turn(rig, thread);
    let input = r#"{"subagent_type":"paper-reader","prompt":"Read."}"#;
    lead(rig, thread, turn, call("toolu_A", "Agent", Some(input)));
    in_background(rig, thread, turn, "toolu_A");
    rig.release_held();
    until_ended(rig, thread);
}

/// With no turn live, a pile of words past the cap holds a second helper's launch, the first
/// helper's landed handback and the costed usage that ends the agent's own turn, each older
/// than words that go. All three stay: the second helper starts, the first ends `Done` with its
/// report, and the agent's own turn ends on the usage (the rig's clock never moves, so no quiet
/// timer ends it).
#[test]
fn words_past_the_cap_leave_a_launch_a_handback_and_the_turn_s_end() {
    let mut rig = Rig::start("held-cap-own-turn", &one_turn("end_turn"));
    let thread = rig.thread();
    first_turn_with_a_helper(&mut rig, thread);
    let input = r#"{"subagent_type":"checker","prompt":"Check.","run_in_background":true}"#;
    let handback = call(
        "toolu_H",
        "SubagentHandback",
        Some(r#"{"message":"Digest."}"#),
    );
    let cost = AgentUpdate::Usage {
        used: Count::from(1u32),
        size: Count::from(10u32),
        cost: Some(Cost {
            amount: Finite::new(0.01).unwrap(),
            currency: "USD".to_owned(),
        }),
    };
    // Opens the agent's own turn, which goes live only once the rig runs.
    feed(&mut rig, thread, filler(0));
    feed(
        &mut rig,
        thread,
        idle(call("toolu_B", "Agent", Some(input))),
    );
    feed(&mut rig, thread, filler(1));
    super::helpers::stamped(&mut rig, thread, Origin::Idle, "toolu_A", handback);
    feed(
        &mut rig,
        thread,
        idle(patch("toolu_H", None, COMPLETED, Some(DELIVERED))),
    );
    feed(&mut rig, thread, filler(2));
    feed(&mut rig, thread, idle(cost));
    for n in 3..12 {
        feed(&mut rig, thread, filler(n));
    }
    let held = own_turn::held_events(&mut rig.actor, thread);
    assert_eq!(
        held.len(),
        3 + 6,
        "launch, handback update, usage and six words"
    );
    rig.run_until("the agent's own turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && actor.work.commits == 0
    });
    let log = rig.log(thread);
    let started: Vec<&str> = log
        .iter()
        .filter_map(|event| {
            let DomainEvent::HelperStarted(started) = event else {
                return None;
            };
            Some(started.acp_id.as_str())
        })
        .collect();
    assert_eq!(started, ["toolu_A", "toolu_B"]);
    let ended: Vec<(HelperOutcome, Option<&TextOrBlob>)> = log
        .iter()
        .filter_map(|event| {
            let DomainEvent::HelperEnded(ended) = event else {
                return None;
            };
            Some((ended.outcome, ended.summary.as_ref()))
        })
        .collect();
    let digest = TextOrBlob::Inline("Digest.".to_owned());
    assert_eq!(ended, [(HelperOutcome::Done, Some(&digest))]);
    let turns = log
        .iter()
        .filter(|event| matches!(event, DomainEvent::TurnEnded(_)))
        .count();
    assert_eq!(turns, 2, "the prompted turn and the agent's own");
    rig.finish();
}

/// With no turn live, a step and a permission card about it, then words past the cap: the step
/// stays, with its location, so the card, once the agent's own turn holds it, names its step
/// (and the card's paths, which it takes from that step, have something to read).
#[test]
fn words_past_the_cap_leave_the_step_a_card_points_at() {
    let mut rig = Rig::start("held-cap-card", &one_turn("end_turn"));
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    let read = AgentUpdate::ToolCall(ToolCall {
        acp_id: "tc-R".into(),
        title: "Read lease.txt".into(),
        kind: ToolKind::Read,
        tool_name: Some("Read".into()),
        status: ToolCallStatus::Pending,
        locations: vec![ToolLocation::Inside {
            path: MatterPath::new("lease.txt").unwrap(),
            line: None,
        }],
        raw_input: None,
        raw_output: None,
        content: Vec::new(),
    });
    let option = |id: &str, kind| PermissionOption {
        option_id: id.into(),
        kind,
        agent_label: id.to_owned(),
    };
    let card = AcpEvent::PermissionRequested {
        session: AgentSessionId::new("sess-1").unwrap(),
        origin: Origin::Idle,
        key: PendingKey(77),
        acp_tool_call_id: "tc-R".into(),
        options: vec![
            option("allow", PermissionKind::AllowOnce),
            option("reject", PermissionKind::RejectOnce),
        ],
        paths: Vec::new(),
        raw_input: None,
    };
    feed(&mut rig, thread, idle(read));
    feed(&mut rig, thread, card);
    for n in 0..12 {
        feed(&mut rig, thread, filler(n));
    }
    assert_eq!(own_turn::waiting(&mut rig.actor, thread), 2 + 6);
    rig.run_until("the card", |actor| {
        let state = &actor.threads[&thread].turn;
        let asked = state
            .agent
            .as_deref()
            .is_some_and(|agent| !agent.permissions.is_empty());
        asked && actor.work.commits == 0
    });
    let log = rig.log(thread);
    let asked = log.iter().find_map(|event| {
        let DomainEvent::PermissionRequested(requested) = event else {
            return None;
        };
        Some(requested.tool_call)
    });
    let step = log.iter().find_map(|event| {
        let DomainEvent::ToolCallStarted(started) = event else {
            return None;
        };
        (started.acp_id == "tc-R").then(|| (started.tool_call, started.locations.clone()))
    });
    let Some((step, locations)) = step else {
        panic!("the step was not logged");
    };
    assert_eq!(asked, Some(Some(step)), "the card names its step");
    assert_eq!(locations.len(), 1, "the step keeps its path");
    rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
    until_ended(&mut rig, thread);
    rig.finish();
}
