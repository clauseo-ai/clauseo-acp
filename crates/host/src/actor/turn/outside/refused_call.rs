//! A tool call that failed as refused on a path in another matter (`agent::matter_fence`):
//! Claude Code's deny rules (a `Read` or an `Edit`, a Windows shell rule) and the sandboxes
//! (Seatbelt, bubblewrap, Codex's) refuse without a permission request, so the host reads the
//! failed call itself and tells the lawyer once a turn for each matter
//! (`outside::tell_other_matter`), naming the innermost other matter one of its paths lies in.
//! Ported from the R18 draft's `denials/tool_call.rs` (its paths and its access-denied test),
//! with Claude Code's own refusal words added. The agent was refused by its own tool already, so
//! no note goes to it: its standing instruction says to tell the lawyer it is another matter's
//! folder (`clauseo_profile`'s legal instructions).
//!
//! A call's paths are kept from its start (its locations, `rawInput`'s paths and its command's
//! path words, `data_fence::candidates`), since a failed update often names none. Paths and
//! text are never logged.
//!
//! Claude Code's words drift between releases (a directory deny reads "File is in a directory
//! that is denied by your permission settings."), so the words are not the only sign: a failed
//! Read or Edit (a Write is one) on a path in a matter the conversation is not invited into is
//! a refusal whatever it says (the deny is what stops it there; a missing file there offering
//! the invite is harmless). The words still count for the rest (a shell call's paths can't all
//! be read). The matter named is only ever one the call's own arguments target: its path
//! argument, or a path word of its command, matched whole name by name as the fence matches
//! (`matter_fence::innermost`). What a call printed never names one: a document in the matter
//! that spells another matter's folder, printed by a failing `grep`, would otherwise have
//! Clauseo offer the lawyer to open that other client's matter.

use std::path::{Path, PathBuf};

use clauseo_protocol::{
    ContentBlock, TextOrBlob, ThreadId, ToolCallContent, ToolCallStatus, ToolKind, ToolLocation,
};

use super::super::super::Actor;
use super::super::current;
use crate::agent::matter_fence::innermost;
use crate::sent::CallFields;

/// Words in a failed call's output that say it was refused: the OS's (`access is denied`,
/// `permission denied`, `operation not permitted`) and Claude Code's permission rules'
/// (`Permission to read … has been denied`, `File is in a directory that is denied by your
/// permission settings.`, `File is covered by a Read deny rule in your permission settings
/// and cannot be edited.`, Claude Code 0.3.293).
const PHRASES: [&str; 7] = [
    "access is denied",
    "access denied",
    "permission denied",
    "operation not permitted",
    "has been denied",
    "was denied",
    "your permission settings",
];

/// Error codes that say the same, as whole words.
const CODES: [&str; 3] = ["error_access_denied", "eacces", "eperm"];

/// A live tool call's start or update (`acp_id`, `fields`, its `raw_output`): its paths are
/// kept, and when it has failed as refused on a path in another matter, the lawyer is told.
pub(in crate::actor::turn) fn seen(
    actor: &mut Actor,
    thread: ThreadId,
    acp_id: &str,
    fields: &CallFields<'_>,
    raw_output: Option<&TextOrBlob>,
) {
    let Some(run) = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.run.as_ref())
    else {
        return;
    };
    let fence = &run.data_deny.matters;
    if fence.others.is_empty() {
        return;
    }
    let own = fence.own.first().cloned().unwrap_or_default();
    let found = paths(fields, &own);
    let failed = fields.status == Some(ToolCallStatus::Failed);
    let refused = failed && says_refused(fields.content, raw_output);
    let Some(current) = current(actor, thread) else {
        return;
    };
    if matches!(fields.kind, Some(ToolKind::Read | ToolKind::Edit)) {
        current.told.file_calls.insert(acp_id.to_owned());
    }
    let file_call = current.told.file_calls.contains(acp_id);
    let kept = current
        .told
        .call_paths
        .entry(acp_id.to_owned())
        .or_default();
    for path in found {
        if !kept.contains(&path) {
            kept.push(path);
        }
    }
    if !failed {
        return;
    }
    let named = current.told.call_paths.remove(acp_id).unwrap_or_default();
    current.told.file_calls.remove(acp_id);
    let Some(run) = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.run.as_ref())
    else {
        return;
    };
    let fence = &run.data_deny.matters;
    let hit = named.iter().find_map(|path| innermost(fence, path));
    if let Some(other) = hit
        && (refused || (file_call && !other.invited))
    {
        super::tell_other_matter(actor, thread, other.id);
    }
}

/// The paths `fields` name: each location (a matter path taken from `own`, the matter
/// folder), then `rawInput`'s (`data_fence::candidates`); a relative one taken from `own`.
fn paths(fields: &CallFields<'_>, own: &Path) -> Vec<PathBuf> {
    let located = fields
        .locations
        .iter()
        .filter_map(|location| match location {
            ToolLocation::Inside { path, .. } => Some(own.join(path.as_str())),
            ToolLocation::Outside { display } => Some(PathBuf::from(display)),
            ToolLocation::Unknown { .. } => None,
        });
    let input = match fields.raw_input {
        Some(TextOrBlob::Inline(text)) => serde_json::from_str(text).ok(),
        Some(TextOrBlob::Blob(_)) | None => None,
    };
    let candidates = crate::agent::data_fence::candidates(&[], input.as_ref(), cfg!(windows));
    located
        .chain(candidates)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                own.join(path)
            }
        })
        .collect()
}

/// Whether a failed call's text content or `rawOutput` says it was refused ([`PHRASES`],
/// [`CODES`]). A blob is not read.
fn says_refused(content: &[ToolCallContent], raw_output: Option<&TextOrBlob>) -> bool {
    texts(content, raw_output).any(|text| says(&text))
}

/// A call's text content and its inline `rawOutput`; a blob is not read.
fn texts<'a>(
    content: &'a [ToolCallContent],
    raw_output: Option<&'a TextOrBlob>,
) -> impl Iterator<Item = String> + 'a {
    let inline = |text: &TextOrBlob| match text {
        TextOrBlob::Inline(text) => Some(text.as_str().to_owned()),
        TextOrBlob::Blob(_) => None,
    };
    let texts = content.iter().filter_map(move |item| match item {
        ToolCallContent::Text { text } => inline(text),
        ToolCallContent::Content {
            content: ContentBlock::Text { text },
        } => Some(text.clone()),
        ToolCallContent::Content { .. }
        | ToolCallContent::Diff { .. }
        | ToolCallContent::Unknown { .. } => None,
    });
    texts.chain(raw_output.and_then(inline))
}

/// Whether `text` holds one of [`PHRASES`], or one of [`CODES`] as a whole word.
fn says(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    if PHRASES.iter().any(|phrase| text.contains(phrase)) {
        return true;
    }
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.split(|c: char| !word(c))
        .any(|part| CODES.contains(&part))
}

#[cfg(test)]
#[path = "refused_call_tests.rs"]
mod tests;
