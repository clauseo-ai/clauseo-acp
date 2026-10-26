use std::path::Path;

use clauseo_protocol::{
    LimitStopReason, MatterPath, RawFrom, TextOrBlob, ToolCallContent, ToolLocation,
};
use serde_json::json;

use super::content::MAX_TEXT_CHUNK_BYTES;
use super::*;
use crate::event::{LimitSignal, LimitStatus};

fn session() -> AgentSessionId {
    AgentSessionId::new("s-1").unwrap()
}

/// Maps the `update` of a `session/update` as the inbound handler does.
fn map_update(mut update: Value, cwd: Option<&Path>) -> Vec<AcpEvent> {
    let quarantined = quarantine_unknown_blocks(&mut update);
    let decoded: acp::SessionUpdate = serde_json::from_value(update).unwrap();
    let mut out = Vec::new();
    super::update(
        &session(),
        Origin::Idle,
        decoded,
        cwd,
        quarantined,
        &mut out,
    );
    out
}

fn only_update(events: Vec<AcpEvent>) -> AgentUpdate {
    let [AcpEvent::Update { update, .. }] = <[AcpEvent; 1]>::try_from(events).unwrap() else {
        panic!("expected one update");
    };
    update
}

#[test]
fn unknown_content_block_kept_raw() {
    let block = json!({"type": "hologram", "beam": [1, 2]});
    let chunk =
        json!({"sessionUpdate": "agent_message_chunk", "messageId": "m-1", "content": block});
    let AgentUpdate::MessageChunk {
        role,
        acp_message_id,
        content,
    } = only_update(map_update(chunk, None))
    else {
        panic!("expected a chunk");
    };
    assert_eq!(role, MessageRole::Answer);
    assert_eq!(acp_message_id.as_deref(), Some("m-1"));
    let ContentBlock::Unsupported {
        kind,
        raw_from,
        raw: Some(TextOrBlob::Inline(raw)),
    } = content
    else {
        panic!("expected an unsupported block, got {content:?}");
    };
    assert_eq!((kind.as_str(), raw_from), ("hologram", RawFrom::Acp));
    assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), block);

    // In a tool call's content, at its index, with the known items around it kept.
    let call = json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "tc-1",
        "title": "Show",
        "content": [
            {"type": "content", "content": {"type": "text", "text": "before"}},
            {"type": "content", "content": block},
            {"type": "content", "content": {"type": "text", "text": "after"}},
        ],
    });
    let AgentUpdate::ToolCall(call) = only_update(map_update(call, None)) else {
        panic!("expected a tool call");
    };
    assert_eq!(call.content.len(), 3);
    assert!(
        matches!(&call.content[0], ToolCallContent::Text { text: TextOrBlob::Inline(t) } if t == "before")
    );
    assert!(matches!(&call.content[1], ToolCallContent::Content {
        content: ContentBlock::Unsupported { kind, .. }
    } if kind == "hologram"));
    assert!(
        matches!(&call.content[2], ToolCallContent::Text { text: TextOrBlob::Inline(t) } if t == "after")
    );
}

#[test]
fn text_chunk_over_1_mib_split() {
    // A 4-byte char straddles the 1 MiB boundary, so the cut must move back to a char boundary.
    let mut text = "a".repeat(MAX_TEXT_CHUNK_BYTES - 2);
    text.push('𝄞');
    text.push_str(&"b".repeat(MAX_TEXT_CHUNK_BYTES + 10));
    let chunk = json!({"sessionUpdate": "agent_thought_chunk", "messageId": "m-2", "content": {"type": "text", "text": text}});
    let events = map_update(chunk, None);
    let mut joined = String::new();
    for event in &events {
        let AcpEvent::Update {
            update:
                AgentUpdate::MessageChunk {
                    role: MessageRole::Thought,
                    acp_message_id: Some(id),
                    content: ContentBlock::Text { text: part },
                },
            ..
        } = event
        else {
            panic!("expected a thought chunk, got {event:?}");
        };
        assert_eq!(id, "m-2");
        assert!(part.len() <= MAX_TEXT_CHUNK_BYTES);
        joined.push_str(part);
    }
    assert_eq!(events.len(), 3);
    assert_eq!(joined, text);

    // Exactly 1 MiB stays one chunk.
    let exact = json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "c".repeat(MAX_TEXT_CHUNK_BYTES)}});
    assert_eq!(map_update(exact, None).len(), 1);
}

#[test]
fn location_outside_cwd() {
    let cwd = Path::new("/matters/lease");
    let inside = |path: &str| ToolLocation::Inside {
        path: MatterPath::new(path).unwrap(),
        line: None,
    };
    let outside = |path: &str| ToolLocation::Outside {
        display: path.to_owned(),
    };
    let at = |path: &str| tool::location(Path::new(path), None, Some(cwd));
    assert_eq!(at("/matters/lease/notes/a.txt"), inside("notes/a.txt"));
    assert_eq!(at("/matters/other/a.txt"), outside("/matters/other/a.txt"));
    assert_eq!(
        at("/matters/lease-2/a.txt"),
        outside("/matters/lease-2/a.txt")
    );
    assert_eq!(
        at("/matters/lease/../other/a.txt"),
        outside("/matters/lease/../other/a.txt")
    );
    assert_eq!(at("notes/a.txt"), inside("notes/a.txt"));
    assert_eq!(at("../a.txt"), outside("../a.txt"));
    // No session folder known: every absolute path is outside.
    let unknown = tool::location(Path::new("/matters/lease/a.txt"), Some(3), None);
    assert_eq!(unknown, outside("/matters/lease/a.txt"));
    // The line survives inside.
    let with_line = tool::location(Path::new("/matters/lease/a.txt"), Some(3), Some(cwd));
    assert_eq!(
        with_line,
        ToolLocation::Inside {
            path: MatterPath::new("a.txt").unwrap(),
            line: Some(3)
        }
    );
}

#[test]
fn location_at_cwd_is_left_out() {
    let call = json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "tc-1",
        "title": "Run",
        "locations": [{"path": "/m"}, {"path": "/m/a.txt"}],
    });
    let AgentUpdate::ToolCall(call) = only_update(map_update(call, Some(Path::new("/m")))) else {
        panic!("expected a tool call");
    };
    assert_eq!(call.locations.len(), 1);
}

#[test]
fn rate_limit_fields_and_unknown_status() {
    let usage = |limit: Value| json!({"sessionUpdate": "usage_update", "used": 1, "size": 2, "_meta": {"_claude/rateLimit": limit}});
    let events = map_update(
        usage(json!({"status": "rejected", "resetsAt": "soon"})),
        None,
    );
    assert_eq!(events.len(), 2);
    let AcpEvent::Limit { signal, .. } = &events[1] else {
        panic!("expected a limit");
    };
    assert_eq!(
        signal,
        &LimitSignal::RateLimit {
            window: None,
            status: LimitStatus::Rejected,
            used_percent: None,
            resets_at: None,
        }
    );
    let events = map_update(usage(json!({"status": "throttled"})), None);
    assert_eq!(
        events[1],
        AcpEvent::Warning(ProtocolWarning::UnknownLimitStatus {
            status: "throttled".to_owned()
        })
    );
    // Other `_meta` keys are never read.
    let plain = json!({"sessionUpdate": "usage_update", "used": 1, "size": 2, "_meta": {"x": 1}});
    assert_eq!(map_update(plain, None).len(), 1);
}

#[test]
fn session_failure_categories() {
    let failure = |category: &str, severity: &str, actions: Value| {
        let meta = json!({"jetbrains": {"air": {"sessionFailure": {
            "id": "f", "revision": 1, "category": category, "severity": severity,
            "title": "T", "actions": actions,
        }}}});
        session_failure(meta.as_object().unwrap()).unwrap()
    };
    let stopped = |reason| {
        Ok(LimitSignal::Stopped {
            reason,
            title: "T".to_owned(),
        })
    };
    assert_eq!(
        failure("limit", "error", json!([])),
        stopped(LimitStopReason::PlanWindow)
    );
    assert_eq!(
        failure("limit", "error", json!(["retry"])),
        stopped(LimitStopReason::Temporary)
    );
    for (category, severity, actions) in [
        ("limit", "error", json!(["new_session"])),
        ("limit", "warning", json!([])),
        ("context", "error", json!([])),
    ] {
        assert_eq!(
            failure(category, severity, actions),
            Err(ProtocolWarning::SessionFailure {
                category: category.to_owned(),
                severity: severity.to_owned(),
                title: "T".to_owned(),
            })
        );
    }
}

#[test]
fn config_option_with_unnameable_id_is_refused() {
    let update = json!({"sessionUpdate": "config_option_update", "configOptions": [
        {"id": "", "name": "Model", "type": "boolean", "currentValue": true}
    ]});
    let events = map_update(update, None);
    assert!(matches!(
        &events[..],
        [AcpEvent::Warning(ProtocolWarning::InvalidUpdate { name, .. })] if name == "config_option_update"
    ));
}
