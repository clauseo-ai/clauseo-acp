//! Prompts, streamed updates, cancel and the turn guard.

use clauseo_acp::{AcpEvent, AgentUpdate, Error, MessageRole, Origin, ProtocolWarning};
use clauseo_protocol::StopReason;

use crate::harness::{self, Ctx, chunk_text, sess1, text, turn};

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
                    && chunk_text(&update).is_some()
                {
                    break;
                }
            }
            ctx.client.cancel(&session).await.unwrap()
        };
        let (end, keys) = futures::join!(prompt, cancel);
        assert!(keys.is_empty());
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
                chunk_text(update),
                Some((MessageRole::Answer, _, Some("m-answer-1")))
            ));
        }
        let _ = AgentUpdate::Plan(Vec::new());
    });
    outcome.assert_clean();
}
