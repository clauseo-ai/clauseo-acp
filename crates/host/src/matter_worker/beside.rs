//! A before-turn checkpoint taken while other turns of the matter run: the papers their tool
//! calls named are their work in progress, not the lawyer's edits outside Clauseo, so the
//! checkpoint's outside edits leave them out. "Named" is what the after-turn checkpoint takes as
//! the turn's own: the places its tool calls wrote ([`super::tool_paths`]) and the files its
//! shell commands write ([`super::writes_in`]). The checkpoint itself holds every file as the
//! folder has it.

use std::collections::BTreeSet;
use std::path::Path;

use clauseo_protocol::{ChangeKind, FileChange};

use crate::sent::TurnPaths;

/// Drops from `changes` each change to a paper one of `running` named in `folder`; a rename
/// goes when either of its paths was named.
pub(super) fn leave_out(folder: &Path, running: &[TurnPaths], changes: &mut Vec<FileChange>) {
    if running.is_empty() || changes.is_empty() {
        return;
    }
    let mut named: BTreeSet<String> = BTreeSet::new();
    for paths in running {
        let written = super::tool_paths(folder, paths);
        named.extend(written.iter().map(|path| path.as_str().to_owned()));
        named.extend(super::writes_in(folder, &paths.shell_writes()));
    }
    changes.retain(|change| {
        let from = match change.change() {
            ChangeKind::Renamed { from } => Some(from.as_str()),
            ChangeKind::Added
            | ChangeKind::Modified
            | ChangeKind::Deleted
            | ChangeKind::Unknown { .. } => None,
        };
        !named.contains(change.path().as_str()) && from.is_none_or(|from| !named.contains(from))
    });
}
