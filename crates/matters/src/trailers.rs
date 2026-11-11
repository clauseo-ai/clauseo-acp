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
//! checkpoint has them, then one `Clauseo-Baseline` and one `Clauseo-Was-Skipped` per file
//! the checkpoint read for the first time (`first_read.rs`), each sorted by path:
//!
//! ```text
//! Clauseo-Baseline: cloud-only notes.txt
//! Clauseo-Was-Skipped: over-size-cap Exhibits/scan 2.pdf
//! ```
//!
//! The value is the reason the file had been skipped, a space, then its path exactly (a path
//! holds no control character, so no line break; spaces in it are kept). Readers take the last
//! paragraph, accept the trailers in any order, ignore keys they do not know, and need only
//! `Clauseo-Checkpoint`.

use std::str::FromStr;

use clauseo_protocol::{
    CheckpointId, CheckpointKind, MatterPath, SkippedFile, SkippedFileReason, ThreadId, TurnId,
};

use crate::first_read::FirstReads;
use crate::types::TimelineEntry;

const CHECKPOINT: &str = "Clauseo-Checkpoint";
const KIND: &str = "Clauseo-Kind";
const THREAD: &str = "Clauseo-Thread";
const TURN: &str = "Clauseo-Turn";
const BASELINE: &str = "Clauseo-Baseline";
const WAS_SKIPPED: &str = "Clauseo-Was-Skipped";

/// The trailer spelling of each skip reason. `Unknown` has none: it is never written.
const REASONS: [(SkippedFileReason, &str); 2] = [
    (SkippedFileReason::CloudOnly, "cloud-only"),
    (SkippedFileReason::OverSizeCap, "over-size-cap"),
];

/// The trailer spelling of each kind. `Unknown` has none: it is never written.
const KINDS: [(CheckpointKind, &str); 7] = [
    (CheckpointKind::Import, "import"),
    (CheckpointKind::BeforeTurn, "before-turn"),
    (CheckpointKind::AfterTurn, "after-turn"),
    (CheckpointKind::OutsideEdits, "outside-edits"),
    (CheckpointKind::Undo, "undo"),
    (CheckpointKind::Restore, "restore"),
    (CheckpointKind::Revisions, "revisions"),
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
        CheckpointKind::Revisions => "Tracked changes",
        CheckpointKind::Unknown => "Checkpoint",
    }
}

/// The commit message for checkpoint `entry` with `files` files changed (for an import: in
/// the tree) and the files it read for the first time, `reads`. `entry.kind` must not be
/// `Unknown`.
pub(crate) fn message(entry: &TimelineEntry, files: usize, reads: &FirstReads) -> String {
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
    let mut text = format!(
        "{words}: {files} {noun}{changed}\n\n{CHECKPOINT}: {}\n{KIND}: {kind}\n{thread}{turn}",
        entry.id
    );
    for (key, list) in [(BASELINE, &reads.baseline), (WAS_SKIPPED, &reads.changed)] {
        for file in list {
            let reason = REASONS
                .iter()
                .find(|(known, _)| *known == file.reason)
                .map_or("unknown", |(_, name)| *name);
            for part in [key, ": ", reason, " ", file.path.as_str(), "\n"] {
                text.push_str(part);
            }
        }
    }
    text
}

/// The files read for the first time that a commit message records, from its last paragraph's
/// `Clauseo-Baseline` and `Clauseo-Was-Skipped` trailers, in the order written. A reason this
/// version does not know reads as `Unknown`; a value without a valid path is left out.
pub(crate) fn first_reads(message: &str) -> FirstReads {
    // Only line breaks are trimmed: a path may end in a space.
    let trimmed = message.trim_end_matches(['\n', '\r']);
    let last = trimmed.rsplit("\n\n").next().unwrap_or(trimmed);
    let mut reads = FirstReads::default();
    for line in last.lines() {
        let Some((key, value)) = line.split_once(": ") else {
            continue;
        };
        let list = match key {
            BASELINE => &mut reads.baseline,
            WAS_SKIPPED => &mut reads.changed,
            _ => continue,
        };
        let Some((reason, path)) = value.split_once(' ') else {
            continue;
        };
        let Ok(path) = MatterPath::new(path.trim_end_matches('\r')) else {
            continue;
        };
        let reason = REASONS
            .iter()
            .find(|(_, name)| *name == reason)
            .map_or(SkippedFileReason::Unknown, |(known, _)| *known);
        list.push(SkippedFile { path, reason });
    }
    reads
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
