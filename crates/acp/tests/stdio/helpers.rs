//! A helper's steps: `claude-helper-background` plays claude-agent-acp 0.110.1's recorded frames.

use clauseo_acp::{AcpEvent, AgentUpdate, Origin};
use clauseo_protocol::{StopReason, ToolLocation};

use crate::harness::{self, Ctx, sess1, text, turn};

/// The lead's `Agent` call that launched the helper.
const AGENT_CALL: &str = "toolu_01Gr3CDaJEng8pyzihUmv3Cq";

/// What an event is, the helper it names and the tool call it is about.
fn shape(event: &AcpEvent) -> (&'static str, Option<&str>, Option<&str>) {
    let tool = |update: &AgentUpdate| match update {
        AgentUpdate::ToolCall(call) => Some(call.acp_id.as_str()),
        AgentUpdate::ToolCallUpdate { acp_id, .. } => Some(acp_id.as_str()),
        _ => None,
    };
    match event {
        AcpEvent::Update { update, .. } => ("update", None, tool(update)),
        AcpEvent::HelperUpdate { helper, update, .. } => {
            ("helper update", Some(helper.as_str()), tool(update))
        }
        AcpEvent::HelperInBackground { helper, .. } => {
            ("in background", Some(helper.as_str()), None)
        }
        other => panic!("unexpected {other:?}"),
    }
}

/// The helper's two `Read` steps and its `SubagentHandback` call reach the client as
/// `HelperUpdate`s naming the `Agent` call, in the lead's turn; the updates of those steps, which
/// the adapter leaves unstamped, and the `Agent` call's own frames are `Update`s; the launch's
/// `isAsync` update gives `HelperInBackground` first.
#[test]
fn a_helper_in_the_background_names_its_steps() {
    let outcome = harness::run("claude-helper-background", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Launch one paper-reader"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        let events = ctx.drain();
        let (read_1, read_2, handback) = (
            "toolu_01WesqZa8jMTM4WMn2ELQNTp",
            "toolu_01VFDnvL4EbFFj1FnZthBh4g",
            "toolu_014ydnQPzgARsFJViooHGimr",
        );
        let helper = Some(AGENT_CALL);
        let lead = Some(AGENT_CALL);
        assert_eq!(
            events.iter().map(shape).collect::<Vec<_>>(),
            [
                ("update", None, lead),
                ("update", None, lead),
                ("update", None, lead),
                ("in background", helper, None),
                ("update", None, lead),
                ("update", None, lead),
                ("helper update", helper, Some(read_1)),
                ("helper update", helper, Some(read_2)),
                ("update", None, Some(read_2)),
                ("update", None, Some(read_1)),
                ("helper update", helper, Some(handback)),
                ("update", None, Some(handback)),
                ("update", None, None),
            ]
        );
        for event in &events {
            let (AcpEvent::Update { origin, .. }
            | AcpEvent::HelperUpdate { origin, .. }
            | AcpEvent::HelperInBackground { origin, .. }) = event
            else {
                panic!("unexpected {event:?}");
            };
            assert_eq!(*origin, Origin::Turn(turn(1)), "{event:?}");
        }
        // The helper's step keeps its place in the matter folder, as a lead's step does.
        let Some(AcpEvent::HelperUpdate {
            update: AgentUpdate::ToolCall(call),
            ..
        }) = events.get(6)
        else {
            panic!("expected the first Read, got {events:?}");
        };
        assert!(
            matches!(
                call.locations.as_slice(),
                [ToolLocation::Inside { path, .. }] if path.as_str() == "lease.txt"
            ),
            "{call:?}"
        );
    });
    outcome.assert_clean();
}
