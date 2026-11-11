//! The note the next prompt starts with after the lawyer undid a step or restored a version.

use std::collections::BTreeSet;

use clauseo_protocol::MatterPath;

/// One input to the note, in the thread's log order.
pub(crate) enum NoteInput<'a> {
    /// A turn started: what came before was told already.
    TurnStarted,
    /// A step was undone; these files went back, or went.
    Undone { restored: &'a [MatterPath] },
    /// A version was restored; these files went back, or went (`RestoreVersion`, held in memory).
    Restored { paths: &'a [MatterPath] },
}

/// The files put back or removed since the last turn started, each once, in the order first
/// seen.
pub(crate) fn paths_since_last_turn<'a>(
    inputs: impl IntoIterator<Item = NoteInput<'a>>,
) -> Vec<MatterPath> {
    let mut paths: Vec<&'a MatterPath> = Vec::new();
    let mut seen: BTreeSet<&'a str> = BTreeSet::new();
    for input in inputs {
        let named = match input {
            NoteInput::TurnStarted => {
                paths.clear();
                seen.clear();
                continue;
            }
            NoteInput::Undone { restored } => restored,
            NoteInput::Restored { paths } => paths,
        };
        for path in named {
            if seen.insert(path.as_str()) {
                paths.push(path);
            }
        }
    }
    paths.into_iter().cloned().collect()
}

/// The note's text for `paths`, or `None` when no file was put back or removed. An undo deletes
/// a file its turn made, and a restore that undoes another deletes one that restore added: the
/// note says so for both, since the version they go back to did not have them.
pub(crate) fn note_text(paths: &[MatterPath]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    let names: Vec<&str> = paths.iter().map(MatterPath::as_str).collect();
    Some(format!(
        "Note from Clauseo: since your last turn, the lawyer put these files back to an earlier \
         version, and removed those it did not have: {}. Read them again before relying on what \
         you remember about them.",
        names.join(", ")
    ))
}

#[cfg(test)]
mod tests;
