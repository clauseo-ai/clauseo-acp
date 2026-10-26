//! Plan limits: Claude's rate limit on `usage_update`, and the AIR `sessionFailure`.

use clauseo_acp::{
    AcpEvent, AgentUpdate, Error, LimitSignal, LimitStatus, MessageRole, Origin, PromptEnd,
    ToolCall,
};
use clauseo_protocol::{
    Count, Finite, LimitStopReason, StopReason, TextOrBlob, Timestamp, ToolCallContent,
    ToolCallStatus, ToolKind,
};

use crate::harness::{self, Ctx, chunk, inside, sess1, text, turn, update_event};

/// Claude's rate limit, as the scenarios send it: five hours, reset at 1792310400 seconds.
fn five_hour(status: LimitStatus, used_percent: f64) -> LimitSignal {
    LimitSignal::RateLimit {
        window: Some("five_hour".to_owned()),
        status,
        used_percent: Some(Finite::new(used_percent).unwrap()),
        resets_at: Some(Timestamp::new(1_792_310_400_000).unwrap()),
    }
}

#[test]
fn rate_limit_warning_typed() {
    let outcome = harness::run("limit-warning", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Check the last clause"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        let t1 = Origin::Turn(turn(1));
        let usage = AgentUpdate::Usage {
            used: Count::new(190_000).unwrap(),
            size: Count::new(200_000).unwrap(),
            cost: None,
        };
        let limit = AcpEvent::Limit {
            session: sess1(),
            origin: t1,
            signal: five_hour(LimitStatus::Warning, 96.0),
        };
        assert_eq!(
            ctx.drain(),
            [
                update_event(
                    t1,
                    chunk(
                        MessageRole::Answer,
                        "m-answer-1",
                        "Checking the last clause. "
                    )
                ),
                update_event(t1, usage),
                limit,
            ]
        );
    });
    outcome.assert_clean();
}

#[test]
fn rate_limit_rejected_typed() {
    let outcome = harness::run("limit-rejected", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Draft clause 5"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        let t1 = Origin::Turn(turn(1));
        let write = AgentUpdate::ToolCall(ToolCall {
            acp_id: "t1-write-draft".into(),
            title: "Edit notes/draft.txt".into(),
            kind: ToolKind::Edit,
            tool_name: Some("Edit".into()),
            status: ToolCallStatus::Completed,
            locations: vec![inside("notes/draft.txt")],
            raw_input: None,
            raw_output: None,
            content: vec![ToolCallContent::Diff {
                path: inside("notes/draft.txt"),
                old: None,
                new: TextOrBlob::Inline("Draft clause\n".to_owned()),
            }],
        });
        let usage = AgentUpdate::Usage {
            used: Count::new(200_000).unwrap(),
            size: Count::new(200_000).unwrap(),
            cost: None,
        };
        assert_eq!(
            ctx.drain(),
            [
                update_event(t1, write),
                update_event(
                    t1,
                    chunk(
                        MessageRole::Answer,
                        "m-answer-1",
                        "Here is the draft clause. "
                    )
                ),
                update_event(t1, usage),
                AcpEvent::Limit {
                    session: sess1(),
                    origin: t1,
                    signal: five_hour(LimitStatus::Rejected, 100.0),
                },
                update_event(
                    t1,
                    chunk(
                        MessageRole::Answer,
                        "m-answer-1",
                        "You've hit your usage limit · resets 3pm"
                    )
                ),
            ]
        );
    });
    outcome.assert_clean();
}

#[test]
fn codex_limit_error_without_session_failure() {
    let outcome = harness::run("limit-codex-error", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Draft clause 5"))
            .await;
        assert_eq!(
            end,
            Err(Error::Rpc {
                code: -32603,
                message: "You've hit your usage limit · resets 3pm".to_owned(),
            })
        );
        // The chunk before the error, and no limit event: the error is not a session failure.
        assert_eq!(
            ctx.drain(),
            [update_event(
                Origin::Turn(turn(1)),
                chunk(
                    MessageRole::Answer,
                    "m-answer-1",
                    "Here is the draft clause. "
                )
            )]
        );
    });
    outcome.assert_clean();
}

#[test]
fn session_failure_typed() {
    let outcome = harness::run("session-failure", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Check the clause"))
            .await
            .unwrap();
        let stopped = LimitSignal::Stopped {
            reason: LimitStopReason::PlanWindow,
            title: "You've hit your usage limit.".to_owned(),
        };
        assert_eq!(
            end,
            PromptEnd {
                stop: StopReason::EndTurn,
                limit: Some(stopped.clone()),
            }
        );
        // The session_info_update carries only the failure: no title event.
        assert_eq!(
            ctx.drain(),
            [
                update_event(
                    Origin::Turn(turn(1)),
                    chunk(MessageRole::Answer, "m-answer-1", "Checking the clause. ")
                ),
                AcpEvent::Limit {
                    session: sess1(),
                    origin: Origin::Turn(turn(1)),
                    signal: stopped,
                },
            ]
        );
    });
    outcome.assert_clean();
}
