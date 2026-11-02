//! Steer into a running turn: `_session/steering` while a `session/prompt` waits, the capability
//! read at `initialize`, and the outcomes the driver maps.

use clauseo_acp::{AcpEvent, AgentUpdate, Error, Origin, SteerResult};
use clauseo_protocol::{SteeringCapability, StopReason};

use crate::harness::{self, Ctx, chunk_text, sess1, text, turn};

/// Waits for the first text chunk of the running turn and returns its event.
async fn first_chunk(ctx: &Ctx) -> AcpEvent {
    loop {
        let event = ctx.next().await;
        if let AcpEvent::Update { update, .. } = &event
            && chunk_text(update).is_some()
        {
            return event;
        }
    }
}

/// The origin of an update event.
fn origin_of(event: &AcpEvent) -> Origin {
    match event {
        AcpEvent::Update { origin, .. } => *origin,
        other => panic!("expected an update, got {other:?}"),
    }
}

/// The text of an update event that is a text chunk.
fn text_of(event: &AcpEvent) -> String {
    match event {
        AcpEvent::Update { update, .. } => match update {
            AgentUpdate::MessageChunk { .. } => chunk_text(update)
                .map(|(_, text, _)| text.to_owned())
                .expect("a text chunk"),
            other => panic!("expected a message chunk, got {other:?}"),
        },
        other => panic!("expected an update, got {other:?}"),
    }
}

/// Steering is advertised by `_meta.steering.supported` and `claudeCode.promptQueueing`.
fn claude_steering() -> SteeringCapability {
    SteeringCapability {
        native: true,
        idle_fallback_to_host: true,
    }
}

#[test]
fn steer_injected_joins_running_turn() {
    let outcome = harness::run("steer-injected", async |ctx: Ctx| {
        let info = ctx.client.initialize().await.unwrap();
        assert_eq!(info.capabilities.steering, Some(claude_steering()));
        ctx.client.new_session(ctx.new_session()).await.unwrap();
        let session = sess1();
        let t1 = turn(1);
        let prompt = ctx.client.prompt(&session, t1, text("Review the clause"));
        let steer = async {
            let first = first_chunk(&ctx).await;
            let result = ctx.client.steer(&session, text("Focus on clause 4")).await;
            (first, result)
        };
        let (end, (first, steered)) = futures::join!(prompt, steer);
        // The steer returns while the prompt still waits: the prompt ends only after it.
        assert_eq!(steered, Ok(SteerResult::Injected));
        assert_eq!(end.unwrap().stop, StopReason::EndTurn);
        // The text after the steer is the running turn's, like the chunk before it.
        assert_eq!(text_of(&first), "The clause ");
        assert_eq!(origin_of(&first), Origin::Turn(t1));
        let rest = ctx.updates();
        assert_eq!(rest.len(), 2, "{rest:?}");
        for (origin, _) in &rest {
            assert_eq!(*origin, Origin::Turn(t1));
        }
        let texts: Vec<_> = rest
            .iter()
            .filter_map(|(_, update)| chunk_text(update).map(|(_, text, _)| text.to_owned()))
            .collect();
        assert_eq!(texts, ["caps liability ", "at the fees paid."]);
    });
    outcome.assert_clean();
}

#[test]
fn steer_prompt_required_then_prompt_with_the_text() {
    let outcome = harness::run("steer-prompt-required", async |ctx: Ctx| {
        let info = ctx.client.initialize().await.unwrap();
        assert_eq!(info.capabilities.steering, Some(claude_steering()));
        ctx.client.new_session(ctx.new_session()).await.unwrap();
        let session = sess1();
        let (t1, t2) = (turn(1), turn(2));
        let prompt = ctx.client.prompt(&session, t1, text("Review the clause"));
        let steer = async {
            let first = first_chunk(&ctx).await;
            let result = ctx.client.steer(&session, text("Focus on clause 4")).await;
            (first, result)
        };
        let (end, (first, steered)) = futures::join!(prompt, steer);
        // The agent ends prompt 1 before it answers the steer: the turn had ended.
        assert_eq!(end.unwrap().stop, StopReason::EndTurn);
        assert_eq!(steered, Ok(SteerResult::PromptRequired));
        assert_eq!(origin_of(&first), Origin::Turn(t1));
        // The host sends the steer text as the next prompt, which runs to its end.
        let end = ctx
            .client
            .prompt(&session, t2, text("Focus on clause 4"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        let updates = ctx.updates();
        let [(origin, update)] = updates.as_slice() else {
            panic!("expected one update, got {updates:?}");
        };
        assert_eq!(*origin, Origin::Turn(t2));
        assert_eq!(
            chunk_text(update).map(|(_, text, _)| text),
            Some("Focusing on clause 4.")
        );
    });
    outcome.assert_clean();
}

#[test]
fn steer_without_fallback_is_unsupported_and_sends_nothing() {
    let outcome = harness::run("steer-fallback", async |ctx: Ctx| {
        let info = ctx.client.initialize().await.unwrap();
        // Native steering without claudeCode's promptQueueing: the host must not steer.
        assert_eq!(
            info.capabilities.steering,
            Some(SteeringCapability {
                native: true,
                idle_fallback_to_host: false,
            })
        );
        ctx.client.new_session(ctx.new_session()).await.unwrap();
        let session = sess1();
        let (t1, t2) = (turn(1), turn(2));
        let prompt = ctx.client.prompt(&session, t1, text("Review the clause"));
        let send_now = async {
            first_chunk(&ctx).await;
            // Nothing reaches the agent: the scenario wants session/cancel next, and would
            // fail on a _session/steering line.
            let steer = ctx.client.steer(&session, text("Focus on clause 4")).await;
            assert_eq!(steer, Err(Error::SteerUnsupported));
            ctx.client.cancel(&session).await.unwrap()
        };
        let (end, keys) = futures::join!(prompt, send_now);
        assert_eq!(keys, Vec::new());
        assert_eq!(end.unwrap().stop, StopReason::Cancelled);
        let end = ctx
            .client
            .prompt(&session, t2, text("Focus on clause 4"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
    });
    outcome.assert_clean();
}

#[test]
fn steer_error_and_odd_outcomes_are_failed() {
    let outcome = harness::run(&harness::local("steer-outcome-failed"), async |ctx: Ctx| {
        ctx.handshake().await;
        let session = sess1();
        let prompt = ctx
            .client
            .prompt(&session, turn(1), text("Review the clause"));
        let steers = async {
            first_chunk(&ctx).await;
            // An error response, an outcome this client does not know, and no outcome at all.
            let mut results = Vec::new();
            for _ in 0..3 {
                results.push(ctx.client.steer(&session, text("Focus on clause 4")).await);
            }
            results
        };
        let (end, results) = futures::join!(prompt, steers);
        assert_eq!(end.unwrap().stop, StopReason::EndTurn);
        assert_eq!(results, vec![Ok(SteerResult::Failed); 3]);
    });
    outcome.assert_clean();
}
