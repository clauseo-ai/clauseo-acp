//! A turn the model refuses as too long to read (`map/too_long.rs`). Claude's refusal and
//! Codex's refusal of an over-long input are live recordings; Codex's full context is written
//! from codex-acp's source. A too-long `sessionFailure` on `end_turn` sets `PromptEnd::too_long`
//! with no limit and no warning; Codex's recorded refusal reads as the internal error it says.

use clauseo_acp::{
    AcpEvent, AgentUpdate, MessageRole, Origin, PromptEnd, ProtocolWarning, TooLong,
};
use clauseo_protocol::{AgentChoiceId, StopReason};

use crate::harness::{self, Ctx, chunk, sess1, text, turn, update_event};

/// Plays `scenario`'s one prompt and returns its end and the events it brought.
fn one_prompt(scenario: &str, codex: bool, check: impl FnOnce(PromptEnd, Vec<AcpEvent>)) {
    let outcome = harness::run(scenario, async |ctx: Ctx| {
        ctx.handshake().await;
        if codex {
            let mode = AgentChoiceId::new("workspace-write").unwrap();
            ctx.client.set_mode(&sess1(), mode).await.unwrap();
        }
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Read the whole lease"))
            .await
            .unwrap();
        check(end, ctx.drain());
    });
    outcome.assert_clean();
}

fn too_long(kind: TooLong) -> PromptEnd {
    PromptEnd {
        stop: StopReason::EndTurn,
        limit: None,
        too_long: Some(kind),
        connection: None,
    }
}

fn no_limit_nor_warning(events: &[AcpEvent]) -> bool {
    !events
        .iter()
        .any(|event| matches!(event, AcpEvent::Limit { .. } | AcpEvent::Warning { .. }))
}

#[test]
fn codex_context_full_ends_too_long() {
    one_prompt("codex-context-full", true, |end, events| {
        assert_eq!(end, too_long(TooLong::ContextExhausted));
        // The session_info_update's copy is its own event, before the answer's.
        assert_eq!(
            events,
            [
                update_event(
                    Origin::Turn(turn(1)),
                    chunk(MessageRole::Answer, "m-answer-1", "Reading the lease. ")
                ),
                AcpEvent::TooLong {
                    session: sess1(),
                    origin: Origin::Turn(turn(1)),
                    kind: TooLong::ContextExhausted,
                },
            ]
        );
    });
}

/// The recorded refusal: a compaction that starts and fails, no answer text, then `end_turn`
/// with the "Prompt is too long" failure of category `service`.
#[test]
fn claude_prompt_too_long_ends_too_long() {
    one_prompt("claude-prompt-too-long", false, |end, events| {
        assert_eq!(end, too_long(TooLong::PromptTooLong));
        assert!(no_limit_nor_warning(&events), "{events:?}");
        let compaction: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AcpEvent::Update {
                    update:
                        update @ (AgentUpdate::CompactionStarted { .. }
                        | AgentUpdate::CompactionEnded { .. }),
                    ..
                } => Some(update.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            compaction,
            [
                AgentUpdate::CompactionStarted {
                    acp_id: "<id-1>".into()
                },
                AgentUpdate::CompactionEnded {
                    acp_id: "<id-1>".into(),
                    completed: false
                },
            ]
        );
    });
}

/// The recorded refusal of a question over Codex's 1,048,576 characters says only "Codex
/// encountered an internal error." (category `service`), which is not read as too long: the
/// prompt ends `EndTurn` and the failure is a warning.
#[test]
fn codex_input_too_long_reads_as_the_internal_error_it_says() {
    one_prompt("codex-input-too-long", true, |end, events| {
        assert_eq!(
            end,
            PromptEnd {
                stop: StopReason::EndTurn,
                limit: None,
                too_long: None,
                connection: None,
            }
        );
        let warnings: Vec<_> = events
            .into_iter()
            .filter_map(|event| match event {
                AcpEvent::Warning { warning, .. } => Some(warning),
                _ => None,
            })
            .collect();
        assert_eq!(
            warnings,
            [ProtocolWarning::SessionFailure {
                category: Some("service".to_owned()),
                severity: Some("error".to_owned()),
                title: Some("Codex encountered an internal error.".to_owned()),
            }]
        );
    });
}
