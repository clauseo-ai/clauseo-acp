//! codex-acp 2.10.0's helper sessions: `codex-helpers` plays recorded frames, once with the
//! connection asking for helper sessions (`ConnectOptions::helper_sessions`) and once without.

use clauseo_acp::{AcpEvent, AgentUpdate, ConnectOptions, Origin, ProtocolWarning};
use clauseo_protocol::{AgentChoiceId, ContentBlock, StopReason, ToolLocation};

use crate::harness::{self, Ctx, inside, sess1, text, turn};

/// The helper's session id in the recording.
const HELPER: &str = "01a20c29-d483-7382-9b0e-1629bbf4cee4";

/// A session id, `helper` for [`HELPER`].
fn name(id: &str) -> &str {
    if id == HELPER { "helper" } else { id }
}

fn kind(update: &AgentUpdate) -> String {
    match update {
        AgentUpdate::MessageChunk {
            content: ContentBlock::Text { text },
            ..
        } => format!("chunk {text:?}"),
        AgentUpdate::ToolCall(call) => format!("tool call {:?}", call.locations),
        AgentUpdate::ToolCallUpdate { .. } => "tool update".to_owned(),
        AgentUpdate::ConfigOptions(_) => "options".to_owned(),
        AgentUpdate::Usage { .. } => "usage".to_owned(),
        other => format!("{other:?}"),
    }
}

/// What an event is, in a line, with its origin.
fn shape(event: &AcpEvent) -> (String, Option<Origin>) {
    match event {
        AcpEvent::Update {
            session,
            origin,
            update,
        } => (
            format!("{} {}", name(session.as_str()), kind(update)),
            Some(*origin),
        ),
        AcpEvent::HelperUpdate {
            session,
            origin,
            helper,
            update,
        } => {
            let line = format!("{} {}: {}", session.as_str(), name(helper), kind(update));
            (line, Some(*origin))
        }
        AcpEvent::HelperSpawned {
            session,
            origin,
            helper,
            parent,
            name: label,
            task,
        } => {
            let line = format!(
                "{} spawned {} by {parent:?}: {label} {task:?}",
                session.as_str(),
                name(helper)
            );
            (line, Some(*origin))
        }
        AcpEvent::HelperAnswer {
            session,
            origin,
            helper,
            content,
        } => {
            let line = format!("{} answer {}: {content:?}", session.as_str(), name(helper));
            (line, Some(*origin))
        }
        AcpEvent::HelperEnded {
            session,
            origin,
            helper,
            state,
        } => {
            let line = format!("{} ended {}: {state:?}", session.as_str(), name(helper));
            (line, Some(*origin))
        }
        AcpEvent::PermissionRequested {
            session, origin, ..
        } => (
            format!("{} permission", name(session.as_str())),
            Some(*origin),
        ),
        AcpEvent::Warning {
            warning: ProtocolWarning::UnknownUpdate { name: update },
            ..
        } => (format!("unknown {update}"), None),
        other => panic!("unexpected {other:?}"),
    }
}

/// Runs the prompt and answers the helper's permission request `allow_once`; returns every
/// event, each in a line with its origin.
async fn prompt_and_allow(ctx: &Ctx) -> Vec<(String, Option<Origin>)> {
    ctx.handshake().await;
    let session = sess1();
    let prompt = ctx
        .client
        .prompt(&session, turn(1), text("Use one sub-agent"));
    let allow = async {
        let seen = ctx
            .until(|event| matches!(event, AcpEvent::PermissionRequested { .. }))
            .await;
        let key = harness::pending_permission(&seen);
        let option = AgentChoiceId::new("allow_once").unwrap();
        ctx.client.answer_permission(key, option).await.unwrap();
        seen
    };
    let (end, mut seen) = futures::join!(prompt, allow);
    assert_eq!(end.unwrap().stop, StopReason::EndTurn);
    seen.extend(ctx.drain());
    seen.iter().map(shape).collect()
}

/// Asking for helper sessions: the helper is announced, its updates, permission request, report
/// and end all come on the lead's session, in the lead's turn; its read is in the session folder.
#[test]
fn asked_for_helper_sessions_the_helper_comes_on_the_lead_session() {
    let options = ConnectOptions {
        helper_sessions: true,
        ..ConnectOptions::default()
    };
    let outcome = harness::run_with("codex-helpers", options, async |ctx: Ctx| {
        let seen = prompt_and_allow(&ctx).await;
        let lines: Vec<&str> = seen.iter().map(|(line, _)| line.as_str()).collect();
        let lease = format!("tool call {:?}", [inside("lease.txt")]);
        assert_eq!(
            lines,
            [
                "sess-1 chunk \".\"".to_owned(),
                "sess-1 spawned helper by None: Read documents None".to_owned(),
                "sess-1 helper: options".to_owned(),
                format!("sess-1 tool call {:?}", Vec::<()>::new()),
                "sess-1 helper: chunk \"I\"".to_owned(),
                format!("sess-1 helper: {lease}"),
                "sess-1 helper: tool update".to_owned(),
                "sess-1 permission".to_owned(),
                "sess-1 helper: usage".to_owned(),
                format!("sess-1 answer helper: {:?}", harness::block("Lease")),
                format!("sess-1 answer helper: {:?}", harness::block(" start")),
                "sess-1 ended helper: Completed".to_owned(),
                "sess-1 tool update".to_owned(),
                "sess-1 chunk \"Lease\"".to_owned(),
            ]
        );
        for (line, origin) in &seen {
            assert_eq!(*origin, Some(Origin::Turn(turn(1))), "{line}");
        }
    });
    outcome.assert_clean();
}

/// Not asking (the default), the same frames map as before: each `subagent_*` update is an
/// unknown update, and the helper's updates and permission request are a session the
/// connection does not know, so `Idle`, with its read outside any folder.
#[test]
fn not_asked_the_same_frames_map_as_before() {
    let outcome = harness::run("codex-helpers", async |ctx: Ctx| {
        let seen = prompt_and_allow(&ctx).await;
        let lines: Vec<&str> = seen.iter().map(|(line, _)| line.as_str()).collect();
        // A session the connection does not know has no folder: the read is outside.
        let display = format!("{}/lease.txt", ctx.cwd.display());
        let lease = format!("tool call {:?}", [ToolLocation::Outside { display }]);
        assert_eq!(
            lines,
            [
                "sess-1 chunk \".\"".to_owned(),
                "unknown subagent_spawned".to_owned(),
                "helper options".to_owned(),
                format!("sess-1 tool call {:?}", Vec::<()>::new()),
                "helper chunk \"I\"".to_owned(),
                format!("helper {lease}"),
                "helper tool update".to_owned(),
                "helper permission".to_owned(),
                "helper usage".to_owned(),
                "helper chunk \"Lease\"".to_owned(),
                "helper chunk \" start\"".to_owned(),
                "unknown subagent_state_update".to_owned(),
                "sess-1 tool update".to_owned(),
                "sess-1 chunk \"Lease\"".to_owned(),
            ]
        );
        for (line, origin) in &seen {
            let expected = if line.starts_with("helper") {
                Some(Origin::Idle)
            } else if line.starts_with("unknown") {
                None
            } else {
                Some(Origin::Turn(turn(1)))
            };
            assert_eq!(*origin, expected, "{line}");
        }
    });
    outcome.assert_clean();
}
