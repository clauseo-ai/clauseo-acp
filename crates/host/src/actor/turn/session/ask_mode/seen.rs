//! What a Claude session's update says about its auto-mode checks, read from the tool result's
//! text as Claude Code 2.1.293 writes it for the model (the tool result's `content`, which
//! claude-agent-acp passes on as the call's content; `is_error` is set). The debug log's
//! `no server verdict for this call (server_not_requested)` never reaches the agent; the tool
//! result reads, by the server's kind (the binary's denial texts):
//!
//! - `server_not_requested`, the request carried no auto-mode context ([`Seen::Withheld`]):
//!   "The server-side auto mode classifier gave no verdict for Edit: the request that produced
//!   this action did not ask for one. Issue the action again once, …". The variant "Edit did not
//!   run. … gave no verdict because it was not shown this call. Repeat the identical Edit call
//!   once now; the repeat will be reviewed as usual." is a call left out of a reviewed request,
//!   so a blip.
//! - any other "… gave no verdict …" ([`Seen::Blip`]): "(error)" (`server_unavailable_error`,
//!   "a hard failure … Don't retry it", and `server_call_unavailable_error`, "a transient
//!   failure … You may try the action again once"), "(timed out)", "(overloaded)" and the rest.
//! - "Permission for this action was denied by the Claude Code auto mode classifier. Reason:
//!   […]" is a judgment, not a missing verdict: nothing.
//!
//! Answers, thoughts and the agent's notices count for the withheld text only, so a blip Claude
//! quotes in its answer changes nothing.

use clauseo_acp::{AgentUpdate, MessageRole};
use clauseo_protocol::{ContentBlock, TextOrBlob, ToolCallContent, ToolCallStatus};

/// What an update says about the auto-mode checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seen {
    /// Refused because the request carried no auto-mode context.
    Withheld,
    /// Refused for want of a verdict otherwise: a service blip.
    Blip,
    /// A tool call completed.
    Done,
}

/// The withheld refusal's text (the tool result's, then the debug log's words should they ever
/// reach the agent).
pub(crate) const WITHHELD: [&str; 3] = [
    "the request that produced this action did not ask for one",
    "no server verdict for this call",
    "server_not_requested",
];

/// Every no-verdict refusal's text (the withheld one too).
pub(crate) const NO_VERDICT: &str = "auto mode classifier gave no verdict";

/// What `update` says about the auto-mode checks, if anything.
pub(crate) fn seen(update: &AgentUpdate) -> Option<Seen> {
    let mut texts: Vec<&str> = Vec::new();
    let status = match update {
        AgentUpdate::ToolCall(call) => {
            tool_texts(&call.content, call.raw_output.as_ref(), &mut texts);
            Some(call.status)
        }
        AgentUpdate::ToolCallUpdate { patch, .. } => {
            let content = patch.content.as_deref().unwrap_or_default();
            tool_texts(content, patch.raw_output.as_ref(), &mut texts);
            patch.status
        }
        AgentUpdate::MessageChunk {
            role: MessageRole::Answer | MessageRole::Thought,
            content: ContentBlock::Text { text },
            ..
        }
        | AgentUpdate::Notice { text, .. } => {
            return withheld(text).then_some(Seen::Withheld);
        }
        AgentUpdate::MessageChunk { .. }
        | AgentUpdate::Plan(_)
        | AgentUpdate::Commands(_)
        | AgentUpdate::CurrentMode { .. }
        | AgentUpdate::ConfigOptions(_)
        | AgentUpdate::SessionTitle { .. }
        | AgentUpdate::Usage { .. }
        | AgentUpdate::CompactionStarted { .. }
        | AgentUpdate::CompactionEnded { .. }
        | AgentUpdate::CompactionSummaryChunk { .. } => return None,
    };
    if texts.iter().any(|text| withheld(text)) {
        Some(Seen::Withheld)
    } else if texts.iter().any(|text| text.contains(NO_VERDICT)) {
        Some(Seen::Blip)
    } else {
        (status == Some(ToolCallStatus::Completed)).then_some(Seen::Done)
    }
}

/// Whether `text` holds the withheld refusal.
fn withheld(text: &str) -> bool {
    WITHHELD.iter().any(|part| text.contains(part))
}

/// A tool call's texts: its content's (inline, or a blob's preview) and its output's.
fn tool_texts<'a>(
    content: &'a [ToolCallContent],
    output: Option<&'a TextOrBlob>,
    texts: &mut Vec<&'a str>,
) {
    for item in content {
        match item {
            ToolCallContent::Text { text } => texts.push(text_of(text)),
            ToolCallContent::Content {
                content: ContentBlock::Text { text },
            } => texts.push(text),
            ToolCallContent::Content { .. }
            | ToolCallContent::Diff { .. }
            | ToolCallContent::Unknown { .. } => {}
        }
    }
    texts.extend(output.map(text_of));
}

fn text_of(text: &TextOrBlob) -> &str {
    match text {
        TextOrBlob::Inline(text) => text,
        TextOrBlob::Blob(blob) => blob.preview(),
    }
}
