//! Builds the event payloads that carry `TextOrBlob` fields, applying the inline/blob rule.
//!
//! ACP hands every `TextOrBlob` over `Inline`. A view entry may carry at most
//! [`INLINE_LIMIT_BYTES`] of inline text, counted as R5 counts it (`TextOrBlob::inline_len`:
//! UTF-8 bytes after JSON escaping, without the quotes). The builders walk an entry's values in
//! R5's field order (the order `ViewEntry::inline_len` documents) and keep each one inline while
//! the running total stays within the limit; a value that does not fit moves into a blob. The
//! blob's bytes come back in [`Built::blobs`] so the caller puts them in the same `Commit` as the
//! event, which makes a blob durable no later than the event naming it.
//!
//! Strings move into the payloads and into blobs without copies, and each value is scanned for
//! its escaped length at most once.

use clauseo_protocol::{
    CompactString, CompactionChanged, CompactionStatus, ContentAppended, ContentBlock, Delivery,
    INLINE_LIMIT_BYTES, MessageId, TextKind, TextOrBlob, ToolCallContent, ToolCallId,
    ToolCallPatch, ToolCallStarted, ToolCallUpdated, TurnId, UserMessage, escaped_len,
};
use clauseo_store::BlobIn;

/// A payload and the blobs its `Blob` values name, which go in the same commit.
#[derive(Debug)]
pub(crate) struct Built<T> {
    /// The payload.
    pub(crate) value: T,
    /// The blobs `value` names, in field order.
    pub(crate) blobs: Vec<BlobIn>,
}

/// What a tool call's view entry costs inline, per field, as last sent. The sent view keeps one
/// per tool call so a `ToolCallUpdated` can recount without the values: a field the patch leaves
/// out keeps its current form, so only its cost matters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ToolFields {
    raw_input: usize,
    raw_output: usize,
    content: usize,
}

impl ToolFields {
    /// The costs of a logged `ToolCallStarted`, for rebuilding the sent view from the log.
    pub(crate) fn of_started(started: &ToolCallStarted) -> Self {
        Self {
            raw_input: optional_len(started.raw_input.as_ref()),
            raw_output: optional_len(started.raw_output.as_ref()),
            content: started.content.iter().map(tool_content_len).sum(),
        }
    }

    /// The costs after a logged patch, for rebuilding the sent view from the log.
    pub(crate) fn patched(self, patch: &ToolCallPatch) -> Self {
        Self {
            raw_input: patch
                .raw_input
                .as_ref()
                .map_or(self.raw_input, TextOrBlob::inline_len),
            raw_output: patch
                .raw_output
                .as_ref()
                .map_or(self.raw_output, TextOrBlob::inline_len),
            content: patch.content.as_ref().map_or(self.content, |content| {
                content.iter().map(tool_content_len).sum()
            }),
        }
    }

    /// The entry's inline total: what `ViewEntry::inline_len` gives for its `ToolCall` entry.
    #[cfg(test)]
    pub(crate) const fn inline_len(self) -> usize {
        self.raw_input + self.raw_output + self.content
    }
}

/// Builds `ToolCallStarted` from ACP's tool call, placing `raw_input`, `raw_output`, then the
/// content items within the limit.
pub(crate) fn tool_call_started(
    turn: TurnId,
    tool_call: ToolCallId,
    call: clauseo_acp::ToolCall,
) -> (Built<ToolCallStarted>, ToolFields) {
    let clauseo_acp::ToolCall {
        acp_id,
        title,
        kind,
        tool_name,
        status,
        locations,
        mut raw_input,
        mut raw_output,
        mut content,
    } = call;
    let mut fit = Fit::new(INLINE_LIMIT_BYTES);
    let fields = ToolFields {
        raw_input: fit.optional(raw_input.as_mut()),
        raw_output: fit.optional(raw_output.as_mut()),
        content: fit.tool_contents(&mut content),
    };
    let value = ToolCallStarted {
        turn,
        tool_call,
        acp_id,
        title,
        kind,
        tool_name,
        status,
        locations,
        raw_input,
        raw_output,
        content,
    };
    (fit.built(value), fields)
}

/// Builds `ToolCallUpdated`, recounting from the fields the entry has after the patch: fields
/// the patch leaves out keep their form and cost from `before`, and the patched ones are placed
/// in field order within what is left of the limit.
pub(crate) fn tool_call_updated(
    tool_call: ToolCallId,
    mut patch: ToolCallPatch,
    before: ToolFields,
) -> (Built<ToolCallUpdated>, ToolFields) {
    let kept = |patched: bool, cost: usize| if patched { 0 } else { cost };
    let fixed = kept(patch.raw_input.is_some(), before.raw_input)
        + kept(patch.raw_output.is_some(), before.raw_output)
        + kept(patch.content.is_some(), before.content);
    let mut fit = Fit::new(INLINE_LIMIT_BYTES.saturating_sub(fixed));
    let fields = ToolFields {
        raw_input: patch
            .raw_input
            .as_mut()
            .map_or(before.raw_input, |value| fit.value(value)),
        raw_output: patch
            .raw_output
            .as_mut()
            .map_or(before.raw_output, |value| fit.value(value)),
        content: patch
            .content
            .as_mut()
            .map_or(before.content, |content| fit.tool_contents(content)),
    };
    (fit.built(ToolCallUpdated { tool_call, patch }), fields)
}

/// Builds `ContentAppended` for an answer or thought chunk whose block is not `Text`.
pub(crate) fn content_appended(
    turn: TurnId,
    message: MessageId,
    acp_message_id: Option<CompactString>,
    kind: TextKind,
    mut block: ContentBlock,
) -> Built<ContentAppended> {
    let mut fit = Fit::new(INLINE_LIMIT_BYTES);
    fit.block(&mut block);
    fit.built(ContentAppended {
        turn,
        message,
        acp_message_id,
        kind,
        block,
    })
}

/// Builds `CompactionChanged`; the summary goes to a blob when it is over the limit.
pub(crate) fn compaction_changed(
    status: CompactionStatus,
    summary: Option<String>,
) -> Built<CompactionChanged> {
    let mut fit = Fit::new(INLINE_LIMIT_BYTES);
    let mut summary = summary.map(TextOrBlob::Inline);
    fit.optional(summary.as_mut());
    fit.built(CompactionChanged { status, summary })
}

/// Builds `UserMessage`, placing the content blocks' `TextOrBlob` fields in order. `Text`
/// blocks are plain strings and cost nothing here (the reducer cuts them into text patches).
pub(crate) fn user_message(
    turn: TurnId,
    message: MessageId,
    acp_message_id: Option<CompactString>,
    mut content: Vec<ContentBlock>,
    delivery: Delivery,
) -> Built<UserMessage> {
    let mut fit = Fit::new(INLINE_LIMIT_BYTES);
    fit.blocks(&mut content);
    fit.built(UserMessage {
        turn,
        message,
        acp_message_id,
        content,
        delivery,
    })
}

/// The running placement of one entry's values: the inline budget left and the blobs made.
struct Fit {
    budget: usize,
    blobs: Vec<BlobIn>,
}

impl Fit {
    const fn new(budget: usize) -> Self {
        Self {
            budget,
            blobs: Vec::new(),
        }
    }

    fn built<T>(self, value: T) -> Built<T> {
        Built {
            value,
            blobs: self.blobs,
        }
    }

    /// Keeps `value` inline when it fits the budget, else moves its text into a blob. Returns
    /// its inline cost after placement (0 for a blob).
    fn value(&mut self, value: &mut TextOrBlob) -> usize {
        let TextOrBlob::Inline(text) = value else {
            return 0;
        };
        // Escaping never shrinks text, so a text longer than the budget needs no scan.
        if text.len() <= self.budget {
            let cost = escaped_len(text);
            if cost <= self.budget {
                self.budget -= cost;
                return cost;
            }
        }
        let blob = BlobIn::new(std::mem::take(text).into_bytes());
        *value = TextOrBlob::Blob(blob.blob_ref());
        self.blobs.push(blob);
        0
    }

    fn optional(&mut self, value: Option<&mut TextOrBlob>) -> usize {
        value.map_or(0, |value| self.value(value))
    }

    /// A content block's `TextOrBlob` fields, in declaration order.
    fn block(&mut self, block: &mut ContentBlock) -> usize {
        match block {
            ContentBlock::Image { data, .. } | ContentBlock::Audio { data, .. } => self.value(data),
            ContentBlock::Resource { text, blob, .. } => {
                self.optional(text.as_mut()) + self.optional(blob.as_mut())
            }
            ContentBlock::Unsupported { raw, .. } => self.optional(raw.as_mut()),
            ContentBlock::Text { .. } | ContentBlock::ResourceLink { .. } => 0,
        }
    }

    fn blocks(&mut self, blocks: &mut [ContentBlock]) -> usize {
        blocks.iter_mut().map(|block| self.block(block)).sum()
    }

    /// A tool call content item's `TextOrBlob` fields, in declaration order.
    fn tool_content(&mut self, content: &mut ToolCallContent) -> usize {
        match content {
            ToolCallContent::Text { text } => self.value(text),
            ToolCallContent::Content { content } => self.block(content),
            ToolCallContent::Diff { old, new, .. } => self.optional(old.as_mut()) + self.value(new),
            ToolCallContent::Unknown { .. } => 0,
        }
    }

    fn tool_contents(&mut self, contents: &mut [ToolCallContent]) -> usize {
        contents
            .iter_mut()
            .map(|item| self.tool_content(item))
            .sum()
    }
}

fn optional_len(value: Option<&TextOrBlob>) -> usize {
    value.map_or(0, TextOrBlob::inline_len)
}

fn block_len(block: &ContentBlock) -> usize {
    match block {
        ContentBlock::Image { data, .. } | ContentBlock::Audio { data, .. } => data.inline_len(),
        ContentBlock::Resource { text, blob, .. } => {
            optional_len(text.as_ref()) + optional_len(blob.as_ref())
        }
        ContentBlock::Unsupported { raw, .. } => optional_len(raw.as_ref()),
        ContentBlock::Text { .. } | ContentBlock::ResourceLink { .. } => 0,
    }
}

fn tool_content_len(content: &ToolCallContent) -> usize {
    match content {
        ToolCallContent::Text { text } => text.inline_len(),
        ToolCallContent::Content { content } => block_len(content),
        ToolCallContent::Diff { old, new, .. } => optional_len(old.as_ref()) + new.inline_len(),
        ToolCallContent::Unknown { .. } => 0,
    }
}

#[cfg(test)]
mod tests;
