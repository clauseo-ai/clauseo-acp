//! When a launch's rules put it in ask mode, what a session's update says about its auto-mode
//! checks (keyed on the tool result's text, as Claude Code 2.1.293 writes it), and the modes a
//! session in ask mode is kept in.

use clauseo_acp::{AgentUpdate, MessageRole, ToolCall};
use clauseo_protocol::{
    CompactString, ContentBlock, NoticeLevel, TextOrBlob, ToolCallContent, ToolCallPatch,
    ToolCallStatus, ToolKind,
};

use super::{Seen, reset_to, seen, too_many_rules};

/// The withheld refusal's tool result (`server_not_requested`, the binary's denial text).
const WITHHELD: &str = "The server-side auto mode classifier gave no verdict for Edit: the request \
     that produced this action did not ask for one. Issue the action again once, as-is; if it \
     is denied again, continue with other tasks that don't require it and tell the user that \
     auto mode could not evaluate it.";

/// Stage C's service blip, verbatim to its "…".
const BLIP: &str = "The server-side auto mode classifier gave no verdict (error), so auto mode \
     cannot determine the safety of Bash. This is a transient failure of the check, not a \
     judgment about the action: a later response may get a verdict. You may try the action \
     again once, as-is. Repeated attempts are slowed by a growing delay, and after 10 responses \
     in a row without a verdict the turn stops.";

/// The hard-failure blip (`server_unavailable_error`).
const HARD: &str = "The server-side auto mode classifier gave no verdict (error), so auto mode \
     cannot determine the safety of Edit. This is a hard failure, not a transient one: the check \
     cannot or does not judge this request, so retrying this action will get the same answer. \
     Don't retry it.";

/// A call left out of a reviewed request: repeated, it is reviewed as usual.
const NOT_SHOWN: &str = "Edit did not run. The server-side auto mode classifier gave no verdict \
     because it was not shown this call; that is not a judgment that the call is unsafe.";

/// A judgment, not a missing verdict.
const DENIED: &str = "Permission for this action was denied by the Claude Code auto mode \
     classifier. Reason: [Unauthorized Access].";

fn failed(text: &str) -> AgentUpdate {
    AgentUpdate::ToolCallUpdate {
        acp_id: CompactString::from("tc-1"),
        patch: ToolCallPatch {
            status: Some(ToolCallStatus::Failed),
            content: Some(vec![ToolCallContent::Content {
                content: ContentBlock::Text {
                    text: text.to_owned(),
                },
            }]),
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
fn the_withheld_tool_result_is_withheld_wherever_it_shows() {
    assert_eq!(seen(&failed(WITHHELD)), Some(Seen::Withheld));
    let output = AgentUpdate::ToolCallUpdate {
        acp_id: CompactString::from("tc-1"),
        patch: ToolCallPatch {
            raw_output: Some(TextOrBlob::Inline(format!("Error: {WITHHELD}"))),
            ..ToolCallPatch::default()
        },
    };
    assert_eq!(seen(&output), Some(Seen::Withheld));
    let call = ToolCall {
        acp_id: CompactString::from("tc-2"),
        title: CompactString::from("Edit memo.md"),
        kind: ToolKind::Edit,
        tool_name: None,
        status: ToolCallStatus::Failed,
        locations: Vec::new(),
        raw_input: None,
        raw_output: None,
        content: vec![ToolCallContent::Text {
            text: TextOrBlob::Inline(WITHHELD.to_owned()),
        }],
    };
    assert_eq!(seen(&AgentUpdate::ToolCall(call)), Some(Seen::Withheld));
    // The debug log's words, should they reach the agent.
    let log = "Bash: no server verdict for this call (server_not_requested)";
    assert_eq!(seen(&chunk(MessageRole::Answer, log)), Some(Seen::Withheld));
    assert_eq!(
        seen(&AgentUpdate::Notice {
            level: NoticeLevel::Warning,
            text: WITHHELD.to_owned(),
        }),
        Some(Seen::Withheld)
    );
}

#[test]
fn any_other_no_verdict_in_a_tool_result_is_a_blip() {
    for text in [BLIP, HARD, NOT_SHOWN] {
        assert_eq!(seen(&failed(text)), Some(Seen::Blip), "{text}");
    }
}

#[test]
fn a_denial_a_quote_or_the_lawyers_words_are_nothing_and_a_completed_call_is_done() {
    assert_eq!(seen(&failed(DENIED)), None);
    // Claude quoting a blip in its answer, or the lawyer's replayed words quoting the refusal.
    assert_eq!(seen(&chunk(MessageRole::Answer, BLIP)), None);
    assert_eq!(seen(&chunk(MessageRole::User, WITHHELD)), None);
    let done = AgentUpdate::ToolCallUpdate {
        acp_id: CompactString::from("tc-3"),
        patch: ToolCallPatch {
            status: Some(ToolCallStatus::Completed),
            ..ToolCallPatch::default()
        },
    };
    assert_eq!(seen(&done), Some(Seen::Done));
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

#[test]
fn over_the_estimate_or_shed_to_fit_the_settings_file_is_ask_mode() {
    let estimated = crate::agent::auto_context::ASK_MODE_AT;
    assert!(!too_many_rules(estimated, false));
    assert!(too_many_rules(estimated + 1, false));
    // Shedding fires near the 2 MiB settings file whatever the estimate says: ask mode anyway,
    // with UC-491 (`at_launch` owes it for both).
    assert!(too_many_rules(400_000, true));
}
