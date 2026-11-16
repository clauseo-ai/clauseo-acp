//! What counts as Claude Code's refusal for want of the auto-mode context, and the modes a
//! session in ask mode is kept in.

use clauseo_acp::{AgentUpdate, MessageRole, ToolCall};
use clauseo_protocol::{
    CompactString, ContentBlock, NoticeLevel, TextOrBlob, ToolCallContent, ToolCallPatch,
    ToolCallStatus, ToolKind,
};

use super::{carries_refusal, reset_to};

/// Claude Code 2.1.293's line for a refused Bash call.
const LINE: &str = "[server-classifier] Bash: no server verdict for this call \
     (server_not_requested); the local classifier decides it";

fn text(text: &str) -> ToolCallContent {
    ToolCallContent::Text {
        text: TextOrBlob::Inline(text.to_owned()),
    }
}

fn update(content: Vec<ToolCallContent>, raw_output: Option<&str>) -> AgentUpdate {
    AgentUpdate::ToolCallUpdate {
        acp_id: CompactString::from("tc-1"),
        patch: ToolCallPatch {
            status: Some(ToolCallStatus::Failed),
            content: Some(content),
            raw_output: raw_output.map(|text| TextOrBlob::Inline(text.to_owned())),
            ..ToolCallPatch::default()
        },
    }
}

fn chunk(role: MessageRole, text: &str) -> AgentUpdate {
    AgentUpdate::MessageChunk {
        role,
        acp_message_id: None,
        content: ContentBlock::Text {
            text: text.to_owned(),
        },
    }
}

#[test]
fn a_tool_result_an_answer_or_a_notice_with_the_refusal_counts() {
    assert!(carries_refusal(&update(vec![text(LINE)], None)));
    assert!(carries_refusal(&update(
        vec![text("Permission denied: server_not_requested")],
        None
    )));
    assert!(carries_refusal(&update(
        Vec::new(),
        Some(r#"{"error":"no server verdict for this call"}"#)
    )));
    let call = ToolCall {
        acp_id: CompactString::from("tc-2"),
        title: CompactString::from("Edit memo.md"),
        kind: ToolKind::Edit,
        tool_name: None,
        status: ToolCallStatus::Failed,
        locations: Vec::new(),
        raw_input: None,
        raw_output: None,
        content: vec![ToolCallContent::Content {
            content: ContentBlock::Text {
                text: LINE.to_owned(),
            },
        }],
    };
    assert!(carries_refusal(&AgentUpdate::ToolCall(call)));
    assert!(carries_refusal(&chunk(MessageRole::Answer, LINE)));
    assert!(carries_refusal(&chunk(MessageRole::Thought, LINE)));
    assert!(carries_refusal(&AgentUpdate::Notice {
        level: NoticeLevel::Warning,
        text: LINE.to_owned(),
    }));
}

#[test]
fn the_lawyers_own_words_and_other_failures_do_not_count() {
    // A replayed question of the lawyer's that quotes it.
    assert!(!carries_refusal(&chunk(MessageRole::User, LINE)));
    assert!(!carries_refusal(&update(
        vec![text("Permission to use Bash has been denied.")],
        None
    )));
    assert!(!carries_refusal(&chunk(
        MessageRole::Answer,
        "No server verdict."
    )));
}

#[test]
fn ask_mode_keeps_default_accept_edits_and_plan_and_sends_auto_back() {
    for mode in ["default", "acceptEdits", "plan"] {
        assert_eq!(reset_to(mode), None, "{mode}");
    }
    for mode in ["auto", "bypassPermissions", "dontAsk", "unknown"] {
        assert_eq!(reset_to(mode), Some("default"), "{mode}");
    }
}
