//! Steps refused because Claude Code's pre-write guard hook could not answer, told in plain
//! words. The guard's entry sets `onFailure: block` (`agent/loads.rs`), so a guard that crashes,
//! times out or ends with another code than 0 or 2 refuses the write or shell command, and
//! claude-agent-acp sends the step as `failed` with Claude Code's text: `PreToolUse:Write hook
//! error: [<the hook's command>]: …` and its own account of the failure. The lawyer reads
//! [`COULD_NOT_CHECK`] in its place, as both the content and the raw output. A step is taken
//! for one only by the guard's own command in that text
//! ([`could_not_answer`](crate::pre_write_guard::could_not_answer)), never by Claude Code's
//! words; a refusal the guard gave (a paper held open) keeps its text. The agent reads what
//! Claude Code tells it, which holds the guard's own line when the guard wrote one.

use clauseo_protocol::{TextOrBlob, ToolCallContent, ToolCallPatch, ToolCallStatus};

use crate::pre_write_guard::{COULD_NOT_CHECK, could_not_answer};

/// Puts [`COULD_NOT_CHECK`] in place of the content and raw output of `patch` when it fails the
/// step because the guard could not answer.
pub(super) fn plain_patch(patch: &mut ToolCallPatch) {
    if patch.status != Some(ToolCallStatus::Failed) {
        return;
    }
    let content = patch.content.as_deref().unwrap_or_default();
    if guard_failed(content, patch.raw_output.as_ref()) {
        patch.content = Some(plain_content());
        patch.raw_output = Some(plain_output());
    }
}

/// [`plain_patch`] for a tool call that starts `failed`.
pub(super) fn plain_call(call: &mut clauseo_acp::ToolCall) {
    if call.status == ToolCallStatus::Failed
        && guard_failed(&call.content, call.raw_output.as_ref())
    {
        call.content = plain_content();
        call.raw_output = Some(plain_output());
    }
}

/// Whether any inline text of `content` or `raw_output` is the guard hook that could not answer.
fn guard_failed(content: &[ToolCallContent], raw_output: Option<&TextOrBlob>) -> bool {
    let texts = content
        .iter()
        .filter_map(|item| match item {
            ToolCallContent::Text { text } => Some(text),
            ToolCallContent::Content { .. }
            | ToolCallContent::Diff { .. }
            | ToolCallContent::Unknown { .. } => None,
        })
        .chain(raw_output);
    texts
        .filter_map(|text| match text {
            TextOrBlob::Inline(text) => Some(text.as_str()),
            TextOrBlob::Blob(_) => None,
        })
        .any(could_not_answer)
}

fn plain_content() -> Vec<ToolCallContent> {
    vec![ToolCallContent::Text {
        text: TextOrBlob::Inline(COULD_NOT_CHECK.to_owned()),
    }]
}

/// [`COULD_NOT_CHECK`] as the compact JSON text a raw output holds.
fn plain_output() -> TextOrBlob {
    // A string always writes as JSON.
    TextOrBlob::Inline(serde_json::to_string(COULD_NOT_CHECK).unwrap_or_default())
}

#[cfg(test)]
mod tests;
