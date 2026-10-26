//! Checkpoint commit messages: a short summary line, then the `Clauseo-*` trailers that let the
//! timeline be rebuilt from the repository alone. Decision record 0007 is the format.
//!
//! ```text
//! After turn: 3 files changed
//!
//! Clauseo-Checkpoint: 0190a6c8-6f8e-7c3a-8b1e-2f4d5a6b7c8d
//! Clauseo-Kind: after-turn
//! Clauseo-Thread: 0190a6c8-6f8e-7c3a-8b1e-2f4d5a6b7c8e
//! Clauseo-Turn: 0190a6c8-6f8e-7c3a-8b1e-2f4d5a6b7c8f
//! ```
//!
//! Writers emit the trailers in that order, `Clauseo-Thread` and `Clauseo-Turn` only when the
//! checkpoint has them. Readers take the last paragraph, accept the trailers in any order, ignore
//! keys they do not know, and need only `Clauseo-Checkpoint`.

use std::str::FromStr;

use clauseo_protocol::{CheckpointId, CheckpointKind, ThreadId, TurnId};

use crate::types::TimelineEntry;

const CHECKPOINT: &str = "Clauseo-Checkpoint";
const KIND: &str = "Clauseo-Kind";
const THREAD: &str = "Clauseo-Thread";
const TURN: &str = "Clauseo-Turn";

/// The trailer spelling of each kind. `Unknown` has none: it is never written.
const KINDS: [(CheckpointKind, &str); 6] = [
    (CheckpointKind::Import, "import"),
    (CheckpointKind::BeforeTurn, "before-turn"),
    (CheckpointKind::AfterTurn, "after-turn"),
    (CheckpointKind::OutsideEdits, "outside-edits"),
    (CheckpointKind::Undo, "undo"),
    (CheckpointKind::Restore, "restore"),
];

/// The trailer value for `kind`; `None` for `Unknown`.
pub(crate) fn kind_name(kind: CheckpointKind) -> Option<&'static str> {
    KINDS
        .iter()
        .find(|(known, _)| *known == kind)
        .map(|(_, name)| *name)
}

/// The kind a trailer value names; `Unknown` for a value from a newer Clauseo.
fn kind_from_name(name: &str) -> CheckpointKind {
    KINDS
        .iter()
        .find(|(_, known)| *known == name)
        .map_or(CheckpointKind::Unknown, |(kind, _)| *kind)
}

/// The summary line's words for `kind`.
fn kind_words(kind: CheckpointKind) -> &'static str {
    match kind {
        CheckpointKind::Import => "Import",
        CheckpointKind::BeforeTurn => "Before turn",
        CheckpointKind::AfterTurn => "After turn",
        CheckpointKind::OutsideEdits => "Edits outside Clauseo",
        CheckpointKind::Undo => "Undo",
        CheckpointKind::Restore => "Restore",
        CheckpointKind::Unknown => "Checkpoint",
    }
}

/// The commit message for checkpoint `entry` with `files` files changed (for an import: in
/// the tree). `entry.kind` must not be `Unknown`.
pub(crate) fn message(entry: &TimelineEntry, files: usize) -> String {
    let noun = if files == 1 { "file" } else { "files" };
    let words = kind_words(entry.kind);
    let changed = if entry.kind == CheckpointKind::Import {
        ""
    } else {
        " changed"
    };
    let kind = kind_name(entry.kind).unwrap_or("unknown");
    let thread = entry
        .thread
        .map(|thread| format!("{THREAD}: {thread}\n"))
        .unwrap_or_default();
    let turn = entry
        .turn
        .map(|turn| format!("{TURN}: {turn}\n"))
        .unwrap_or_default();
    format!(
        "{words}: {files} {noun}{changed}\n\n{CHECKPOINT}: {}\n{KIND}: {kind}\n{thread}{turn}",
        entry.id
    )
}

/// The checkpoint a commit message describes, from its last paragraph's trailers; `None` when
/// it has no valid `Clauseo-Checkpoint`. A missing or unreadable kind is `Unknown`; a thread or
/// turn that does not parse is left out.
pub(crate) fn parse(message: &str) -> Option<TimelineEntry> {
    let trimmed = message.trim_end();
    let last = trimmed.rsplit("\n\n").next().unwrap_or(trimmed);
    let mut id = None;
    let mut kind = CheckpointKind::Unknown;
    let mut thread = None;
    let mut turn = None;
    for line in last.lines() {
        let Some((key, value)) = line.split_once(": ") else {
            continue;
        };
        let value = value.trim();
        match key {
            CHECKPOINT => id = CheckpointId::from_str(value).ok(),
            KIND => kind = kind_from_name(value),
            THREAD => thread = ThreadId::from_str(value).ok(),
            TURN => turn = TurnId::from_str(value).ok(),
            _ => {}
        }
    }
    Some(TimelineEntry {
        id: id?,
        kind,
        thread,
        turn,
    })
}

#[cfg(test)]
mod tests;
