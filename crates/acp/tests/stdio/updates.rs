//! Session updates that carry state: plans, session state, usage, notices, unknown updates.

use clauseo_acp::{AcpEvent, AgentUpdate, MessageRole, Origin, ProtocolWarning};
use clauseo_protocol::{
    AgentChoiceId, AvailableCommand, ConfigOptionKind, ConfigValue, Cost, Count, Finite,
    NoticeLevel, PlanEntry, PlanPriority, PlanStatus, StopReason,
};

use crate::harness::{self, Ctx, block, chunk, sess1, text, turn, update_event};

/// A `tool_call` with no content and no raw input or output.
fn plan_entry(content: &str, priority: PlanPriority, status: PlanStatus) -> PlanEntry {
    PlanEntry {
        content: content.to_owned(),
        priority,
        status,
    }
}

#[test]
fn plan_replaced_full_list() {
    let outcome = harness::run("plan", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Plan the review"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        let t1 = Origin::Turn(turn(1));
        let (high, medium) = (PlanPriority::High, PlanPriority::Medium);
        let (done, doing, todo) = (
            PlanStatus::Completed,
            PlanStatus::InProgress,
            PlanStatus::Pending,
        );
        let indemnity = "Read the indemnity clause";
        let cap = "Check the liability cap";
        let summary = "Summarise for the client";
        assert_eq!(
            ctx.updates(),
            [
                (
                    t1,
                    AgentUpdate::Plan(vec![
                        plan_entry(indemnity, high, doing),
                        plan_entry(cap, high, todo),
                        plan_entry(summary, medium, todo),
                    ]),
                ),
                (
                    t1,
                    AgentUpdate::Plan(vec![
                        plan_entry(indemnity, high, done),
                        plan_entry(cap, high, doing),
                        plan_entry(summary, medium, todo),
                    ]),
                ),
                (
                    t1,
                    AgentUpdate::Plan(vec![
                        plan_entry(indemnity, high, done),
                        plan_entry(cap, high, done),
                        plan_entry(summary, medium, done),
                    ]),
                ),
                (
                    t1,
                    chunk(MessageRole::Answer, "m-answer-1", "The plan is done.")
                ),
            ]
        );
    });
    outcome.assert_clean();
}

#[test]
fn session_state_and_idle_update() {
    let outcome = harness::run("session-state", async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        let end = ctx
            .client
            .prompt(&session, turn(1), text("Set up the review"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        let t1 = Origin::Turn(turn(1));
        let commands = vec![
            AvailableCommand {
                name: "review".into(),
                description: "Review the document for risky clauses".to_owned(),
                input_hint: None,
            },
            AvailableCommand {
                name: "summarise".into(),
                description: "Summarise the open document".to_owned(),
                input_hint: None,
            },
        ];
        assert_eq!(
            ctx.updates(),
            [
                (t1, AgentUpdate::Commands(commands)),
                (
                    t1,
                    AgentUpdate::CurrentMode {
                        mode_id: "plan".into()
                    }
                ),
                (
                    t1,
                    AgentUpdate::SessionTitle {
                        title: Some("Lease review".to_owned())
                    }
                ),
            ]
        );
        let option = AgentChoiceId::new("model").unwrap();
        let value = ConfigValue::Select(AgentChoiceId::new("deep").unwrap());
        let options = ctx
            .client
            .set_config_option(&session, option, value)
            .await
            .unwrap();
        assert_eq!(options.len(), 1);
        assert!(matches!(
            &options[0].kind,
            ConfigOptionKind::Select { current_value, .. } if current_value == "deep"
        ));
        // The agent sends this update after its response, between turns.
        let AcpEvent::Update { origin, update, .. } = ctx.next().await else {
            panic!("expected the config options update");
        };
        assert_eq!(origin, Origin::Idle);
        assert_eq!(update, AgentUpdate::ConfigOptions(options));
    });
    outcome.assert_clean();
}

#[test]
fn usage_maps_cost() {
    let outcome = harness::run("usage", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Is it standard?"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        let usage = AgentUpdate::Usage {
            used: Count::new(1520).unwrap(),
            size: Count::new(200_000).unwrap(),
            cost: Some(Cost {
                amount: Finite::new(0.0123).unwrap(),
                currency: "USD".to_owned(),
            }),
        };
        assert_eq!(
            ctx.updates(),
            [
                (
                    Origin::Turn(turn(1)),
                    chunk(MessageRole::Answer, "m-answer-1", "The clause is standard.")
                ),
                (Origin::Turn(turn(1)), usage),
            ]
        );
    });
    outcome.assert_clean();
}

#[test]
fn notice_and_compaction() {
    let outcome = harness::run("notice-compaction", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Keep going"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        let t1 = Origin::Turn(turn(1));
        let summary = |text: &str| AgentUpdate::CompactionSummaryChunk {
            acp_id: "c-1".into(),
            content: block(text),
        };
        assert_eq!(
            ctx.updates(),
            [
                (
                    t1,
                    AgentUpdate::Notice {
                        level: NoticeLevel::Warning,
                        text: "Context is nearly full\n\nOlder turns will be summarised."
                            .to_owned(),
                    }
                ),
                (
                    t1,
                    AgentUpdate::CompactionStarted {
                        acp_id: "c-1".into()
                    }
                ),
                (t1, summary("The lease was reviewed in March. ")),
                (t1, summary("Clause 4 caps the liability.")),
                (
                    t1,
                    AgentUpdate::CompactionEnded {
                        acp_id: "c-1".into(),
                        completed: true,
                    }
                ),
                (
                    t1,
                    chunk(
                        MessageRole::Answer,
                        "m-answer-1",
                        "The earlier turns are summarised."
                    )
                ),
            ]
        );
    });
    outcome.assert_clean();
}

#[test]
fn unknown_update_ignored() {
    let outcome = harness::run("unknown-update", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Check the clause"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        let t1 = Origin::Turn(turn(1));
        assert_eq!(
            ctx.drain(),
            [
                update_event(
                    t1,
                    chunk(
                        MessageRole::Answer,
                        "m-answer-1",
                        "Before the unknown update. "
                    )
                ),
                AcpEvent::Warning {
                    session: Some(sess1()),
                    warning: ProtocolWarning::UnknownUpdate {
                        name: "_clauseo_test_unknown".to_owned(),
                    },
                },
                update_event(
                    t1,
                    chunk(
                        MessageRole::Answer,
                        "m-answer-1",
                        "After the unknown update."
                    )
                ),
            ]
        );
    });
    outcome.assert_clean();
}
