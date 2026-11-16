//! The entries a matter folder may not hold while a Claude agent works there
//! ([`refuse_claude_entries`]), checked before its launch and again before each of its prompts
//! and session calls (`codex::prepare::recheck`, the same re-check Codex's `.codex` gets).
//!
//! One list ([`CLAUDE_REFUSED_ENTRIES`]), one code per entry, so a later entry adds a row and
//! its own code, as Codex's refusal does.

use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;

use crate::error::Error;

/// An entry the matter folder may not hold while a Claude agent works there: its path below the
/// matter folder, one name per folder, the code of the refusal and its words (for the log and
/// the turn's detail; the lawyer's words come from the code,
/// `ui/components/src/lib/words-errors.ts`).
pub(crate) struct RefusedEntry {
    pub(crate) path: &'static [&'static str],
    pub(crate) code: ErrorCode,
    words: &'static str,
}

/// The entries [`refuse_claude_entries`] refuses, checked in this order; Clauseo doesn't start
/// Claude in a matter folder that holds one:
/// - `.claude/scheduled_tasks.json`.
/// - `.git` (a folder, or a `.git` file). Only the matter folder's own: a parent's is not
///   checked here.
pub(crate) const CLAUDE_REFUSED_ENTRIES: [RefusedEntry; 2] = [
    RefusedEntry {
        path: &[".claude", "scheduled_tasks.json"],
        code: ErrorCode::MatterClaudeTasksFile,
        words: "Claude can't work in this matter while its folder holds \
                \".claude/scheduled_tasks.json\". Move, rename or remove that file, then try \
                again.",
    },
    RefusedEntry {
        path: &[".git"],
        code: ErrorCode::MatterGitFolder,
        words: "Claude can't work in this matter while its folder holds a \".git\" folder. \
                Move or rename that \".git\" folder, then try again.",
    },
];

/// A Claude agent is refused in a matter folder holding any of [`CLAUDE_REFUSED_ENTRIES`], of
/// any kind (a file, a folder, a link or junction wherever it points), before it starts and
/// before each of its prompts and session calls (the entry may appear later). `folder` is the
/// matter folder as resolved through links. A folder on the way that is a file (`.claude` as a
/// file) holds no entry. A stat that fails for any other reason fails closed, as
/// `matter.folder_missing` (the folder may be a share that does not answer); so does a matter
/// folder that is gone or is not a folder.
pub(crate) fn refuse_claude_entries(folder: &Path) -> Result<(), Error> {
    for refused in &CLAUDE_REFUSED_ENTRIES {
        let name = refused.path.join("/");
        // `symlink_metadata`: a link at the entry's own name counts too, wherever it points.
        match std::fs::symlink_metadata(entry_path(folder, refused.path)) {
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) => {}
            Err(error) => {
                tracing::warn!(entry = %name, "the stat of an entry failed");
                return Err(folder_failed(
                    Some(error.kind()),
                    "the matter folder's entry stat failed",
                ));
            }
            Ok(_) => {
                tracing::warn!(
                    entry = %name,
                    "a Claude session was refused: the matter folder holds a refused entry"
                );
                return Err(Error::refused(refused.code, refused.words));
            }
        }
    }
    match std::fs::metadata(folder) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(folder_failed(None, "the matter folder is not a folder")),
        Err(error) => Err(folder_failed(
            Some(error.kind()),
            "the matter folder's stat failed",
        )),
    }
}

/// `path`'s names joined onto `folder` one by one, never as one piece: under a verbatim folder
/// (`\\?\C:\…`, as a stored long path is) a `/` is no separator but a character no name may
/// hold, and the stat would fail every time.
pub(crate) fn entry_path(folder: &Path, path: &[&str]) -> PathBuf {
    path.iter()
        .fold(folder.to_path_buf(), |joined, name| joined.join(name))
}

/// The fail-closed refusal of a Claude session whose matter folder could not be checked: `why`
/// and the io error's kind go to the log, never the path or the error's text (either may hold
/// the user's name).
fn folder_failed(kind: Option<std::io::ErrorKind>, why: &str) -> Error {
    tracing::warn!(error_kind = ?kind, "a Claude session was refused: {why}");
    Error::refused(
        ErrorCode::MatterFolderMissing,
        "Clauseo couldn't check the matter's folder for Claude.",
    )
}

#[cfg(test)]
mod tests;
