//! Tool calls: kinds, statuses, content and locations.

use std::path::{Component, Path};

use agent_client_protocol::schema::v1 as acp;
use clauseo_protocol::{
    CompactString, ContentBlock, MatterPath, TextOrBlob, ToolCallContent, ToolCallPatch,
    ToolCallStatus, ToolKind, ToolLocation,
};
use serde_json::Value;

use crate::event::ToolCall;
use crate::map::content;

/// A `tool_call`. `quarantined` holds the blocks of unknown type by content index.
pub(crate) fn tool_call(
    call: acp::ToolCall,
    cwd: Option<&Path>,
    quarantined: Vec<(usize, ContentBlock)>,
) -> ToolCall {
    ToolCall {
        acp_id: CompactString::from(&*call.tool_call_id.0),
        title: call.title.into(),
        kind: kind(call.kind),
        tool_name: call.name.map(Into::into),
        status: status(call.status),
        locations: locations(call.locations, cwd),
        raw_input: call.raw_input.as_ref().and_then(raw),
        raw_output: call.raw_output.as_ref().and_then(raw),
        content: contents(call.content, cwd, quarantined),
    }
}

/// A `tool_call_update`'s fields: each one present replaces the call's.
pub(crate) fn patch(
    fields: acp::ToolCallUpdateFields,
    cwd: Option<&Path>,
    quarantined: Vec<(usize, ContentBlock)>,
) -> ToolCallPatch {
    ToolCallPatch {
        title: fields.title.map(Into::into),
        kind: fields.kind.map(kind),
        tool_name: fields.name.map(Into::into),
        status: fields.status.map(status),
        locations: fields.locations.map(|items| locations(items, cwd)),
        raw_input: fields.raw_input.as_ref().and_then(raw),
        raw_output: fields.raw_output.as_ref().and_then(raw),
        content: fields
            .content
            .map(|items| contents(items, cwd, quarantined)),
    }
}

fn kind(kind: acp::ToolKind) -> ToolKind {
    match kind {
        acp::ToolKind::Read => ToolKind::Read,
        acp::ToolKind::Edit => ToolKind::Edit,
        acp::ToolKind::Delete => ToolKind::Delete,
        acp::ToolKind::Move => ToolKind::Move,
        acp::ToolKind::Search => ToolKind::Search,
        acp::ToolKind::Execute => ToolKind::Execute,
        acp::ToolKind::Think => ToolKind::Think,
        acp::ToolKind::Fetch => ToolKind::Fetch,
        acp::ToolKind::SwitchMode => ToolKind::SwitchMode,
        // A kind newer than schema 1.11.0 is other.
        acp::ToolKind::Other | _ => ToolKind::Other,
    }
}

fn status(status: acp::ToolCallStatus) -> ToolCallStatus {
    match status {
        acp::ToolCallStatus::Pending => ToolCallStatus::Pending,
        acp::ToolCallStatus::Completed => ToolCallStatus::Completed,
        acp::ToolCallStatus::Failed => ToolCallStatus::Failed,
        // A status newer than schema 1.11.0 counts as running.
        acp::ToolCallStatus::InProgress | _ => ToolCallStatus::InProgress,
    }
}

/// Raw input or output as compact JSON text.
fn raw(value: &Value) -> Option<TextOrBlob> {
    // Writing a `Value` to a string cannot fail: its keys are strings.
    serde_json::to_string(value).ok().map(TextOrBlob::Inline)
}

/// The content list. Terminal content is dropped: Clauseo advertises no terminal, so an agent
/// that sends one anyway has nothing a client could show.
fn contents(
    items: Vec<acp::ToolCallContent>,
    cwd: Option<&Path>,
    quarantined: Vec<(usize, ContentBlock)>,
) -> Vec<ToolCallContent> {
    let mut quarantined = quarantined.into_iter().peekable();
    let mut mapped = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        if let Some((_, block)) = quarantined.next_if(|(at, _)| *at == index) {
            mapped.push(ToolCallContent::Content { content: block });
            continue;
        }
        match item {
            acp::ToolCallContent::Content(item) => {
                mapped.push(match content::block(item.content) {
                    ContentBlock::Text { text } => ToolCallContent::Text {
                        text: TextOrBlob::Inline(text),
                    },
                    block @ (ContentBlock::Image { .. }
                    | ContentBlock::Audio { .. }
                    | ContentBlock::ResourceLink { .. }
                    | ContentBlock::Resource { .. }
                    | ContentBlock::Unsupported { .. }) => {
                        ToolCallContent::Content { content: block }
                    }
                });
            }
            acp::ToolCallContent::Diff(diff) => mapped.push(ToolCallContent::Diff {
                path: path_location(&diff.path, None, cwd).unwrap_or_else(|| outside(&diff.path)),
                old: diff.old_text.map(TextOrBlob::Inline),
                new: TextOrBlob::Inline(diff.new_text),
            }),
            // Terminal content, or a kind newer than schema 1.11.0.
            acp::ToolCallContent::Terminal(_) | _ => {}
        }
    }
    mapped
}

/// The locations, without any that is the session folder itself, which is no file a location
/// could name (a matter path is never empty) and is not outside the matter either.
fn locations(items: Vec<acp::ToolCallLocation>, cwd: Option<&Path>) -> Vec<ToolLocation> {
    items
        .into_iter()
        .filter_map(|item| {
            let at_cwd = cwd.is_some_and(|cwd| item.path == cwd);
            (!at_cwd).then(|| location(&item.path, item.line, cwd))
        })
        .collect()
}

/// A place a tool touches, by lexical comparison with the session's `cwd`: inside it a matter
/// path, else `Outside` with the path as display text. No file system call.
pub(crate) fn location(path: &Path, line: Option<u32>, cwd: Option<&Path>) -> ToolLocation {
    path_location(path, line, cwd).unwrap_or_else(|| outside(path))
}

fn path_location(path: &Path, line: Option<u32>, cwd: Option<&Path>) -> Option<ToolLocation> {
    let relative = if path.is_absolute() {
        path.strip_prefix(cwd?).ok()?
    } else {
        path
    };
    Some(ToolLocation::Inside {
        path: matter_path(relative)?,
        line,
    })
}

/// `relative`'s parts joined with `/`, when each is a plain name and the result is a valid
/// matter path.
fn matter_path(relative: &Path) -> Option<MatterPath> {
    let mut joined = String::new();
    for part in relative.components() {
        let Component::Normal(part) = part else {
            return None;
        };
        if !joined.is_empty() {
            joined.push('/');
        }
        joined.push_str(part.to_str()?);
    }
    MatterPath::new(&joined).ok()
}

fn outside(path: &Path) -> ToolLocation {
    ToolLocation::Outside {
        display: path.display().to_string(),
    }
}
