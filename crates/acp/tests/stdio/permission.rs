//! Permission requests and elicitations: answers, refusals and cancel.

use clauseo_acp::{AcpEvent, AgentUpdate, Error, MessageRole, Origin, PendingKey, ToolCall};
use clauseo_protocol::{
    AgentChoiceId, ElicitationRequest, ElicitationResponse, PermissionKind, PermissionOption,
    StopReason, ToolCallPatch, ToolCallStatus,
};
use serde_json::json;

use crate::harness::{self, Ctx, chunk, is_chunk, sess1, text, turn};

/// The four options each request of `permission-kinds` offers, in the scenario's order.
fn offered() -> Vec<PermissionOption> {
    [
        ("allow-once", PermissionKind::AllowOnce, "Allow once"),
        ("allow-always", PermissionKind::AllowAlways, "Always allow"),
        ("reject-once", PermissionKind::RejectOnce, "Reject"),
        (
            "reject-always",
            PermissionKind::RejectAlways,
            "Always reject",
        ),
    ]
    .into_iter()
    .map(|(id, kind, label)| PermissionOption {
        option_id: id.into(),
        kind,
        agent_label: label.to_owned(),
    })
    .collect()
}

/// Waits for the next permission request, checks which tool call it is about and what it
/// offers, and returns its key. Nothing is answered.
async fn next_permission(ctx: &Ctx, tool: &str) -> PendingKey {
    let seen = ctx
        .until(|event| matches!(event, AcpEvent::PermissionRequested { .. }))
        .await;
    let [
        AcpEvent::PermissionRequested {
            key,
            acp_tool_call_id,
            options,
            ..
        },
    ] = seen.as_slice()
    else {
        panic!("expected one permission request, got {seen:?}");
    };
    assert_eq!(acp_tool_call_id.as_str(), tool);
    assert_eq!(options, &offered());
    *key
}

/// Answers with `option`, a permission request's option id.
async fn answer(ctx: &Ctx, key: PendingKey, option: &str) {
    let option = AgentChoiceId::new(option).unwrap();
    ctx.client.answer_permission(key, option).await.unwrap();
}

/// Answers perm-2 to perm-4 of `permission-kinds` as the scenario expects them.
async fn answer_rest(ctx: &Ctx) {
    let key = next_permission(ctx, "tc-perm-2").await;
    answer(ctx, key, "allow-always").await;
    let key = next_permission(ctx, "tc-perm-3").await;
    answer(ctx, key, "reject-once").await;
    let key = next_permission(ctx, "tc-perm-4").await;
    answer(ctx, key, "reject-always").await;
}

#[test]
fn permission_kinds_answered() {
    let outcome = harness::run("permission-kinds", async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        let prompt = ctx
            .client
            .prompt(&session, turn(1), text("Edit four clauses"));
        let answers = async {
            let key = next_permission(&ctx, "tc-perm-1").await;
            answer(&ctx, key, "allow-once").await;
            answer_rest(&ctx).await;
        };
        let (end, ()) = futures::join!(prompt, answers);
        assert_eq!(end.unwrap().stop, StopReason::EndTurn);
        assert_eq!(
            ctx.updates(),
            [(
                Origin::Turn(turn(1)),
                chunk(
                    MessageRole::Answer,
                    "m-answer-1",
                    "Four edits were decided."
                )
            )]
        );
    });
    outcome.assert_clean();
}

#[test]
fn permission_option_must_be_offered() {
    let outcome = harness::run("permission-kinds", async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        let prompt = ctx
            .client
            .prompt(&session, turn(1), text("Edit four clauses"));
        let answers = async {
            let key = next_permission(&ctx, "tc-perm-1").await;
            let refused = ctx
                .client
                .answer_permission(key, AgentChoiceId::new("allow-forever").unwrap())
                .await;
            assert_eq!(refused, Err(Error::UnknownOption));
            // Still pending: the answer with an offered option is delivered.
            answer(&ctx, key, "allow-once").await;
            answer_rest(&ctx).await;
        };
        let (end, ()) = futures::join!(prompt, answers);
        assert_eq!(end.unwrap().stop, StopReason::EndTurn);
    });
    outcome.assert_clean();
}

#[test]
fn permission_answer_twice_not_pending() {
    let outcome = harness::run("permission-kinds", async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        let prompt = ctx
            .client
            .prompt(&session, turn(1), text("Edit four clauses"));
        let answers = async {
            let key = next_permission(&ctx, "tc-perm-1").await;
            answer(&ctx, key, "allow-once").await;
            let again = ctx
                .client
                .answer_permission(key, AgentChoiceId::new("allow-once").unwrap())
                .await;
            assert_eq!(again, Err(Error::PermissionNotPending));
            answer_rest(&ctx).await;
        };
        let (end, ()) = futures::join!(prompt, answers);
        assert_eq!(end.unwrap().stop, StopReason::EndTurn);
    });
    outcome.assert_clean();
}

#[test]
fn permission_cancelled_on_cancel() {
    let outcome = harness::run("permission-cancelled", async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        let prompt = ctx.client.prompt(&session, turn(1), text("Edit brief.txt"));
        let cancel = async {
            let seen = ctx
                .until(|event| matches!(event, AcpEvent::PermissionRequested { .. }))
                .await;
            let key = harness::pending_permission(&seen);
            let keys = ctx.client.cancel(&session).await.unwrap();
            (seen, key, keys)
        };
        let (end, (seen, key, keys)) = futures::join!(prompt, cancel);
        assert_eq!(keys, [key]);
        assert_eq!(end.unwrap().stop, StopReason::Cancelled);
        let [call, request] = seen.as_slice() else {
            panic!("expected a tool call then the request, got {seen:?}");
        };
        let AcpEvent::Update {
            origin: Origin::Turn(_),
            update: AgentUpdate::ToolCall(ToolCall { acp_id, .. }),
            ..
        } = call
        else {
            panic!("expected the tool call first, got {call:?}");
        };
        assert_eq!(acp_id, "t-edit-1");
        assert!(matches!(request, AcpEvent::PermissionRequested { .. }));
        // The update the agent sends after the cancel is tagged Late.
        assert_eq!(
            ctx.updates(),
            [(
                Origin::Late(turn(1)),
                AgentUpdate::ToolCallUpdate {
                    acp_id: "t-edit-1".into(),
                    patch: ToolCallPatch {
                        status: Some(ToolCallStatus::Failed),
                        ..ToolCallPatch::default()
                    },
                },
            )]
        );
    });
    outcome.assert_clean();
}

#[test]
fn unknown_permission_kind_rejected() {
    let outcome = harness::run(
        &harness::local("unknown-permission-kind"),
        async |ctx: Ctx| {
            ctx.handshake().await;
            let end = ctx
                .client
                .prompt(&sess1(), turn(1), text("Edit brief.txt"))
                .await
                .unwrap();
            assert_eq!(end.stop, StopReason::EndTurn);
            let events = ctx.drain();
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, AcpEvent::PermissionRequested { .. })),
                "{events:?}"
            );
        },
    );
    outcome.assert_clean();
}

#[test]
fn elicitation_form_answered() {
    let outcome = harness::run("elicitation-form", async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        let prompt = ctx.client.prompt(&session, turn(1), text("Who is liable?"));
        let answer = async {
            let seen = ctx
                .until(|event| matches!(event, AcpEvent::ElicitationRequested { .. }))
                .await;
            let [
                chunk_event,
                AcpEvent::ElicitationRequested {
                    key,
                    elicitation_id,
                    request,
                    ..
                },
            ] = seen.as_slice()
            else {
                panic!("expected a chunk then the form, got {seen:?}");
            };
            assert!(is_chunk(chunk_event));
            assert_eq!(elicitation_id, &None);
            let schema = json!({
                "type": "object",
                "properties": {"party": {"type": "string", "title": "Liable party"}},
                "required": ["party"]
            });
            assert_eq!(
                request,
                &ElicitationRequest::Form {
                    message: "Which party is liable for the cap?".to_owned(),
                    schema: serde_json::from_value(schema).unwrap(),
                }
            );
            let values = serde_json::from_value(json!({"party": "Tenant"})).unwrap();
            ctx.client
                .answer_elicitation(*key, ElicitationResponse::Accept { values })
                .await
                .unwrap();
        };
        let (end, ()) = futures::join!(prompt, answer);
        assert_eq!(end.unwrap().stop, StopReason::EndTurn);
    });
    outcome.assert_clean();
}

#[test]
fn elicitation_url_completed() {
    let outcome = harness::run("elicitation-url", async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        let prompt = ctx.client.prompt(&session, turn(1), text("Sign it"));
        let answer = async {
            let seen = ctx
                .until(|event| matches!(event, AcpEvent::ElicitationRequested { .. }))
                .await;
            let [
                _,
                AcpEvent::ElicitationRequested {
                    key,
                    elicitation_id,
                    request,
                    ..
                },
            ] = seen.as_slice()
            else {
                panic!("expected a chunk then the page, got {seen:?}");
            };
            assert_eq!(elicitation_id.as_deref(), Some("el-1"));
            assert_eq!(
                request,
                &ElicitationRequest::Url {
                    message: "Sign the contract in your browser".to_owned(),
                    url: "https://sign.example.com/contract/42".to_owned(),
                }
            );
            let values = serde_json::from_value(json!({})).unwrap();
            ctx.client
                .answer_elicitation(*key, ElicitationResponse::Accept { values })
                .await
                .unwrap();
            // The agent sends this once the page is done, before the prompt ends.
            let done = ctx
                .until(|event| matches!(event, AcpEvent::ElicitationCompleted { .. }))
                .await;
            assert_eq!(
                done.last(),
                Some(&AcpEvent::ElicitationCompleted {
                    elicitation_id: "el-1".to_owned()
                })
            );
        };
        let (end, ()) = futures::join!(prompt, answer);
        assert_eq!(end.unwrap().stop, StopReason::EndTurn);
    });
    outcome.assert_clean();
}
