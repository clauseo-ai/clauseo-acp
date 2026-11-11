//! A turn the model refuses as too long to read (`map/too_long.rs`), on the placeholder
//! scenarios built from the adapters' source until the live recordings replace them: a
//! `sessionFailure` on `end_turn` sets `PromptEnd::too_long` with no limit and no event, and
//! Codex's refusal of an over-long input is an RPC error that `Error::prompt_too_long` reads.

use clauseo_acp::{AcpEvent, Error, MessageRole, Origin, PromptEnd};
use clauseo_protocol::{AgentChoiceId, StopReason};

use crate::harness::{self, Ctx, chunk, sess1, text, turn, update_event};

/// The scenario's one prompt ends `EndTurn` flagged too long, with no limit, and the only event
/// is the answer so far: no limit event and no warning.
fn ends_too_long(scenario: &str, codex: bool) {
    let outcome = harness::run(scenario, async |ctx: Ctx| {
        ctx.handshake().await;
        if codex {
            let ask = AgentChoiceId::new("workspace-write").unwrap();
            ctx.client.set_mode(&sess1(), ask).await.unwrap();
        }
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Read the whole lease"))
            .await
            .unwrap();
        assert_eq!(
            end,
            PromptEnd {
                stop: StopReason::EndTurn,
                limit: None,
                too_long: true,
            }
        );
        assert_eq!(
            ctx.drain(),
            [update_event(
                Origin::Turn(turn(1)),
                chunk(MessageRole::Answer, "m-answer-1", "Reading the lease. ")
            )]
        );
    });
    outcome.assert_clean();
}

#[test]
fn codex_context_full_ends_too_long() {
    ends_too_long("codex-context-full", true);
}

#[test]
fn claude_prompt_too_long_ends_too_long() {
    ends_too_long("claude-prompt-too-long", false);
}

/// Codex's refusal of input over 1,048,576 characters comes as an RPC error, before any update:
/// `prompt_too_long` holds, and the error's text holds no data.
#[test]
fn codex_input_too_long_is_a_too_long_rpc_error() {
    let outcome = harness::run("codex-input-too-long", async |ctx: Ctx| {
        ctx.handshake().await;
        let ask = AgentChoiceId::new("workspace-write").unwrap();
        ctx.client.set_mode(&sess1(), ask).await.unwrap();
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Read the whole lease"))
            .await;
        let Err(error @ Error::Rpc { .. }) = end else {
            panic!("expected an RPC error, got {end:?}");
        };
        assert!(error.prompt_too_long(), "{error:?}");
        assert_eq!(
            error.to_string(),
            "the agent answered session/prompt with error -32603: Internal error"
        );
        let events: Vec<AcpEvent> = ctx.drain();
        assert_eq!(events, [], "no event before the refusal");
    });
    outcome.assert_clean();
}
