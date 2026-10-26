//! Prompts, streamed updates, cancel and the turn guard.

use clauseo_acp::{AcpEvent, Error, MessageRole, Origin, PromptEnd, ProtocolWarning};
use clauseo_protocol::{AgentChoiceId, StopReason};
use futures::future::{Either, select};

use crate::harness::{self, Ctx, chunk, is_chunk, sess1, text, turn, update_event};

#[test]
fn cancel_late_updates_tagged_late() {
    let outcome = harness::run("cancel-late-updates", async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        let (t1, t2) = (turn(1), turn(2));
        let prompt = ctx.client.prompt(&session, t1, text("Summarise"));
        let cancel = async {
            // Wait for the first chunk so the agent is past its prompt step.
            loop {
                if let AcpEvent::Update { update, .. } = ctx.next().await
                    && harness::chunk_text(&update).is_some()
                {
                    break;
                }
            }
            ctx.client.cancel(&session).await.unwrap()
        };
        let (end, keys) = futures::join!(prompt, cancel);
        assert_eq!(keys, Vec::new());
        assert_eq!(end.unwrap().stop, StopReason::Cancelled);
        let end = ctx
            .client
            .prompt(&session, t2, text("Again"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        let updates = ctx.updates();
        let origins: Vec<_> = updates.iter().map(|(origin, _)| *origin).collect();
        assert_eq!(
            origins,
            [
                Origin::Late(t1),
                Origin::Late(t1),
                Origin::Late(t1),
                Origin::Late(t1),
                Origin::Turn(t2)
            ]
        );
    });
    outcome.assert_clean();
}

#[test]
fn agent_exit_fails_pending_prompt() {
    let outcome = harness::run("exit-mid-turn", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx.client.prompt(&sess1(), turn(1), text("Go")).await;
        assert_eq!(end, Err(Error::Closed));
        let updates = ctx.updates();
        assert_eq!(updates.len(), 1);
    });
    assert_eq!(outcome.driver, Err(Error::Closed));
    assert_eq!(outcome.exit, Some(1));
}

#[test]
fn malformed_line_skipped() {
    let outcome = harness::run("malformed-line", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Go"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        let events = ctx.drain();
        assert_eq!(events.len(), 3, "{events:?}");
        assert!(matches!(
            &events[1],
            AcpEvent::Warning(ProtocolWarning::MalformedLine { .. })
        ));
        for index in [0, 2] {
            let AcpEvent::Update { update, origin, .. } = &events[index] else {
                panic!()
            };
            assert_eq!(*origin, Origin::Turn(turn(1)));
            assert!(matches!(
                harness::chunk_text(update),
                Some((MessageRole::Answer, _, Some("m-answer-1")))
            ));
        }
    });
    outcome.assert_clean();
}

#[test]
fn prompt_stream_text_events() {
    let outcome = harness::run("prompt-stream", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Summarise"))
            .await
            .unwrap();
        assert_eq!(
            end,
            PromptEnd {
                stop: StopReason::EndTurn,
                limit: None
            }
        );
        let t1 = Origin::Turn(turn(1));
        let thought = MessageRole::Thought;
        let answer = MessageRole::Answer;
        assert_eq!(
            ctx.updates(),
            [
                (t1, chunk(thought, "m-thought-1", "Reading the ")),
                (t1, chunk(thought, "m-thought-1", "indemnity clause ")),
                (t1, chunk(thought, "m-thought-1", "first.")),
                (t1, chunk(answer, "m-answer-1", "The clause ")),
                (t1, chunk(answer, "m-answer-1", "caps liability ")),
                (t1, chunk(answer, "m-answer-1", "at the fees ")),
                (t1, chunk(answer, "m-answer-1", "paid in the ")),
                (t1, chunk(answer, "m-answer-1", "last twelve months.")),
            ]
        );
    });
    outcome.assert_clean();
}

#[test]
fn second_prompt_turn_running() {
    let outcome = harness::run("prompt-stream", async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        // The agent ends this turn without waiting for the client, so a prompt sent after the
        // first chunk could find the turn already over. Queuing both in one poll keeps the first
        // turn in flight when the driver reads the second.
        let first = ctx.client.prompt(&session, turn(1), text("Summarise"));
        let second = ctx.client.prompt(&session, turn(2), text("Again"));
        let (first, second) = futures::join!(first, second);
        assert_eq!(second, Err(Error::TurnRunning));
        assert_eq!(first.unwrap().stop, StopReason::EndTurn);
        // Nothing reached the agent for the second prompt: assert_clean checks it.
        assert_eq!(ctx.updates().len(), 8);
    });
    outcome.assert_clean();
}

#[test]
fn two_prompts_in_order() {
    let outcome = harness::run("queue-two-prompts", async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        let (t1, t2) = (turn(1), turn(2));
        let first = ctx.client.prompt(&session, t1, text("Edit clause 4"));
        let answer = async {
            let seen = ctx
                .until(|event| matches!(event, AcpEvent::PermissionRequested { .. }))
                .await;
            let key = harness::pending_permission(&seen);
            let option = AgentChoiceId::new("allow-once").unwrap();
            ctx.client.answer_permission(key, option).await.unwrap();
            seen
        };
        let (first, seen) = futures::join!(first, answer);
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(first.unwrap().stop, StopReason::EndTurn);
        let second = ctx
            .client
            .prompt(&session, t2, text("Second question"))
            .await
            .unwrap();
        assert_eq!(second.stop, StopReason::EndTurn);
        assert_eq!(
            ctx.updates(),
            [
                (
                    Origin::Turn(t1),
                    chunk(MessageRole::Answer, "m-answer-1", "Clause 4 is fine. ")
                ),
                (
                    Origin::Turn(t2),
                    chunk(MessageRole::Answer, "m-answer-2", "Second answer.")
                ),
            ]
        );
    });
    outcome.assert_clean();
}

#[test]
fn cancel_then_prompt() {
    let outcome = harness::run("send-now", async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        let (t1, t2) = (turn(1), turn(2));
        let first = ctx.client.prompt(&session, t1, text("Cap the liability?"));
        let cancel = async {
            let seen = ctx.until(is_chunk).await;
            let keys = ctx.client.cancel(&session).await.unwrap();
            (seen, keys)
        };
        let (first, (seen, keys)) = futures::join!(first, cancel);
        assert_eq!(keys, Vec::new());
        assert_eq!(first.unwrap().stop, StopReason::Cancelled);
        assert_eq!(
            seen,
            [update_event(
                Origin::Turn(t1),
                chunk(
                    MessageRole::Answer,
                    "m-answer-1",
                    "The clause caps liability at "
                )
            )]
        );
        let second = ctx
            .client
            .prompt(&session, t2, text("Use the 2019 version instead"))
            .await
            .unwrap();
        assert_eq!(second.stop, StopReason::EndTurn);
        assert_eq!(
            ctx.updates(),
            [(
                Origin::Turn(t2),
                chunk(
                    MessageRole::Answer,
                    "m-answer-2",
                    "Using the 2019 version. "
                )
            )]
        );
    });
    outcome.assert_clean();
}

#[test]
fn dropped_prompt_future_is_harmless() {
    let outcome = harness::run("cancel-unanswered", async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        let t1 = turn(1);
        let prompt = Box::pin(ctx.client.prompt(&session, t1, text("Summarise")));
        let cancel = Box::pin(async {
            let seen = ctx.until(is_chunk).await;
            let keys = ctx.client.cancel(&session).await.unwrap();
            (seen, keys)
        });
        // The agent never answers the prompt: once the cancel is sent, drop the prompt.
        let ((seen, keys), prompt) = match select(prompt, cancel).await {
            Either::Left(_) => panic!("the agent never answers the prompt"),
            Either::Right((answered, prompt)) => (answered, prompt),
        };
        drop(prompt);
        assert_eq!(keys, Vec::new());
        assert_eq!(
            seen,
            [update_event(
                Origin::Turn(t1),
                chunk(MessageRole::Answer, "m-answer-1", "The clause ")
            )]
        );
        // Dropping the client now ends the connection; the agent exits on stdin close.
    });
    outcome.assert_clean();
}

#[test]
fn fs_and_terminal_refused() {
    let outcome = harness::run(&harness::local("fs-terminal-refused"), async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Read the brief"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        assert_eq!(ctx.drain(), Vec::new());
    });
    outcome.assert_clean();
}
