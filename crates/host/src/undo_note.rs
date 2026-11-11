//! The note the next prompt starts with after the lawyer undid a step or restored a version.
//! What the lawyer did is told in order: an Undo that removed a file and a Versions restore that
//! brought it back are two lines, the restore last, so the agent knows the file is back.

use clauseo_protocol::{MatterPath, Timestamp};

use crate::connectors::civil_from_days;

/// One input to the note from the thread's log, in log order.
pub(crate) enum NoteInput<'a> {
    /// A turn started: what came before was told already.
    TurnStarted,
    /// A step was undone at `at`; these files went back, or went.
    Undone {
        restored: &'a [MatterPath],
        at: Timestamp,
    },
}

/// A Versions restore of the thread's matter (`RestoreVersion`, held in memory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RestoreNote {
    /// The files it wrote: back as the version had them.
    pub(crate) back: Vec<MatterPath>,
    /// The files it removed, which the version did not have.
    pub(crate) removed: Vec<MatterPath>,
    /// When it was done.
    pub(crate) at: Timestamp,
}

/// One thing the note tells, in the order the lawyer did it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Told {
    /// Undo put this file back, or removed it.
    Undone(MatterPath),
    /// A Versions restore.
    Restored(RestoreNote),
}

/// What happened since the last turn started: the log's undos, with `restores` (held in
/// memory, each after the log's last `TurnStarted`) put among them by time.
pub(crate) fn since_last_turn<'a>(
    inputs: impl IntoIterator<Item = NoteInput<'a>>,
    restores: Vec<RestoreNote>,
) -> Vec<Told> {
    let mut told = Vec::new();
    let mut restores = restores.into_iter().peekable();
    for input in inputs {
        match input {
            NoteInput::TurnStarted => told.clear(),
            NoteInput::Undone { restored, at } => {
                while let Some(restore) = restores.next_if(|restore| restore.at < at) {
                    told.push(Told::Restored(restore));
                }
                for path in restored {
                    add_undone(&mut told, path);
                }
            }
        }
    }
    told.extend(restores.map(Told::Restored));
    told
}

/// Adds an undo of `path`, unless the note already tells it as undone since anything else
/// touched it: the log and the host's memory can both name one undo.
pub(crate) fn add_undone(told: &mut Vec<Told>, path: &MatterPath) {
    let last = told.iter().rev().find(|item| match item {
        Told::Undone(undone) => undone == path,
        Told::Restored(restore) => restore.back.contains(path) || restore.removed.contains(path),
    });
    if !matches!(last, Some(Told::Undone(_))) {
        told.push(Told::Undone(path.clone()));
    }
}

/// The note's text for `told`, or `None` when nothing was put back or removed. Undos in a row
/// share one line, each restore has its own, in order. An undo deletes a file its turn made,
/// and a restore that undoes another deletes one that restore added: the undo line says so,
/// since the version they go back to did not have them.
pub(crate) fn note_text(told: &[Told]) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut undone: Vec<&str> = Vec::new();
    for item in told {
        match item {
            Told::Undone(path) => undone.push(path.as_str()),
            Told::Restored(restore) => {
                undo_line(&mut lines, &mut undone);
                restore_lines(&mut lines, restore);
            }
        }
    }
    undo_line(&mut lines, &mut undone);
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "{} Read them again before relying on what you remember about them.",
        lines.join("\n")
    ))
}

/// The line for the undos in `undone`, which it empties; the first line starts the note.
fn undo_line(lines: &mut Vec<String>, undone: &mut Vec<&str>) {
    if undone.is_empty() {
        return;
    }
    let lead = if lines.is_empty() {
        "Note from Clauseo: since your last turn, the lawyer"
    } else {
        "Then the lawyer"
    };
    lines.push(format!(
        "{lead} put these files back to an earlier version, and removed those it did not have: \
         {}.",
        undone.join(", ")
    ));
    undone.clear();
}

/// The lines for one restore: the files it brought back, then those it removed.
fn restore_lines(lines: &mut Vec<String>, restore: &RestoreNote) {
    let time = utc_minute(restore.at);
    if !restore.back.is_empty() {
        let back = if restore.back.len() == 1 {
            "it's"
        } else {
            "they're"
        };
        let line = format!(
            "The lawyer restored {} from Versions; {back} back as of {time}.",
            names(&restore.back)
        );
        lines.push(lead(lines, line));
    }
    if !restore.removed.is_empty() {
        let line = if restore.back.is_empty() {
            format!(
                "The lawyer restored an earlier version from Versions as of {time}; it removed \
                 {}, which that version did not have.",
                names(&restore.removed)
            )
        } else {
            format!(
                "That restore also removed {}, which that version did not have.",
                names(&restore.removed)
            )
        };
        lines.push(lead(lines, line));
    }
}

/// `line`, starting the note when it is the first.
fn lead(lines: &[String], line: String) -> String {
    if lines.is_empty() {
        format!("Note from Clauseo: {line}")
    } else {
        line
    }
}

fn names(paths: &[MatterPath]) -> String {
    let names: Vec<&str> = paths.iter().map(MatterPath::as_str).collect();
    names.join(", ")
}

/// `at` to the minute in UTC, as `2026-11-11 14:05 UTC`: the host knows no time zone here, and
/// the agent can tell UTC from its own clock.
fn utc_minute(at: Timestamp) -> String {
    let secs = at.as_millis().div_euclid(1000);
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let minutes = secs.rem_euclid(86_400) / 60;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        minutes / 60,
        minutes % 60
    )
}

#[cfg(test)]
mod tests;
