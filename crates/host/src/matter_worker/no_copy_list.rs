//! The session's no-copy list, kept up to date after each before-turn checkpoint: the files
//! Clauseo holds no copy of, which the pre-write guard refuses to let the agent delete or
//! overwrite (`pre_write_guard/no_copy.rs`). The launch made the file with its header alone and
//! pinned its identity in the hook command, so it is never replaced: the changes since the list
//! as it reads now are appended, an `add` or a `remove` line each, in one write, through the
//! same checks the guard reads it with (a regular file with one link, the identity the launch
//! made, reached through no link). Off the actor: on the matter's thread, before the turn's
//! `TurnStarted`, so the agent's first tool call meets this turn's list.

use std::io::Write as _;

use clauseo_protocol::SkippedFile;

use crate::log_file::Cause;
use crate::pre_write_guard::no_copy::{List, NoCopyList};

/// Appends to `list`, if the session has one, what makes it name `files`; a failure is a
/// warning, and the guard still looks at each target itself.
pub(super) fn update(list: Option<&NoCopyList>, files: &[SkippedFile]) {
    let Some(list) = list else {
        return;
    };
    let changes = List::read(list).changes(files);
    if changes.is_empty() {
        return;
    }
    let written = crate::write::open_append_record(&list.file, list.id)
        .and_then(|mut file| file.write_all(changes.as_bytes()));
    if let Err(error) = written {
        tracing::warn!(
            error = %Cause(&error),
            "the guard's no-copy list was not updated; the guard looks at each file itself"
        );
    }
}

#[cfg(test)]
mod tests;
