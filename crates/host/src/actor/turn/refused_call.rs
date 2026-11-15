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

use std::path::{Path, PathBuf};

use clauseo_protocol::{
    ContentBlock, TextOrBlob, ThreadId, ToolCallContent, ToolCallStatus, ToolLocation,
};

use super::super::Actor;
use super::current;
use crate::agent::matter_fence::innermost;
use crate::sent::CallFields;

/// Words in a failed call's output that say it was refused: the OS's (`access is denied`,
/// `permission denied`, `operation not permitted`) and Claude Code's permission rules'
/// (`Permission to read … has been denied`).
const PHRASES: [&str; 6] = [
    "access is denied",
    "access denied",
    "permission denied",
    "operation not permitted",
    "has been denied",
    "was denied",
];

/// Error codes that say the same, as whole words.
const CODES: [&str; 3] = ["error_access_denied", "eacces", "eperm"];

/// A live tool call's start or update (`acp_id`, `fields`, its `raw_output`): its paths are
/// kept, and when it has failed as refused on a path in another matter, the lawyer is told.
pub(super) fn seen(
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
    let refused =
        fields.status == Some(ToolCallStatus::Failed) && says_refused(fields.content, raw_output);
    let Some(current) = current(actor, thread) else {
        return;
    };
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
    if !refused {
        return;
    }
    let named = current.told.call_paths.remove(acp_id).unwrap_or_default();
    let Some(run) = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.run.as_ref())
    else {
        return;
    };
    let fence = &run.data_deny.matters;
    let matter = named
        .iter()
        .find_map(|path| innermost(fence, path).map(|other| other.id));
    if let Some(matter) = matter {
        super::outside::tell_other_matter(actor, thread, matter);
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
    let inline = |text: &TextOrBlob| match text {
        TextOrBlob::Inline(text) => Some(text.as_str().to_owned()),
        TextOrBlob::Blob(_) => None,
    };
    let texts = content.iter().filter_map(|item| match item {
        ToolCallContent::Text { text } => inline(text),
        ToolCallContent::Content {
            content: ContentBlock::Text { text },
        } => Some(text.clone()),
        ToolCallContent::Content { .. }
        | ToolCallContent::Diff { .. }
        | ToolCallContent::Unknown { .. } => None,
    });
    texts
        .chain(raw_output.and_then(inline))
        .any(|text| says(&text))
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
