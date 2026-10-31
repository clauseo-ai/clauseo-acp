//! The note the next prompt starts with after the lawyer undid a step or restored a version.

use std::collections::BTreeSet;

use clauseo_protocol::MatterPath;

/// One input to the note, in the thread's log order.
pub(crate) enum NoteInput<'a> {
    /// A turn started: what came before was told already.
    TurnStarted,
    /// A step was undone; these files went back.
    Undone { restored: &'a [MatterPath] },
    /// A version was restored; these files went back. `RestoreVersion` (a later command)
    /// makes these; until then only the tests do.
    #[cfg(test)]
    Restored { paths: &'a [MatterPath] },
}

/// The files put back since the last turn started, each once, in the order first seen.
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
            #[cfg(test)]
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

/// The note's text for `paths`, or `None` when no file was put back.
pub(crate) fn note_text(paths: &[MatterPath]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    let names: Vec<&str> = paths.iter().map(MatterPath::as_str).collect();
    Some(format!(
        "Note from Clauseo: since your last turn, the lawyer put these files back to an earlier \
         version: {}. Read them again before relying on what you remember about them.",
        names.join(", ")
    ))
}

#[cfg(test)]
mod tests;
